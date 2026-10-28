//! cmux (<https://cmux.com>), a macOS terminal built for running many coding
//! agents, driven through its `cmux` CLI.
//!
//! What am relies on (cmux 0.64, checked against its source):
//! - A cmux terminal exports `TERM_PROGRAM=ghostty` plus `CMUX_WORKSPACE_ID`,
//!   `CMUX_SURFACE_ID` and `CMUX_SOCKET_PATH`; cmux's own docs detect "in
//!   cmux" by `CMUX_WORKSPACE_ID` ([`inside`]).
//! - The control socket's default mode (`cmuxOnly`) only answers the cmux CLI
//!   run by a process started from a cmux terminal (by process ancestry).
//!   Run elsewhere, every socket command fails with a broken pipe, so the
//!   socket is only used [`inside`] cmux. `cmux <path>` opens a directory
//!   without the socket and works from anywhere ([`open_path`]).
//! - `cmux rpc <method> [json]` prints the v2 response object: `window.list`
//!   → `{"windows":[{"id"}]}`, `workspace.list {"window_id"}` →
//!   `{"workspaces":[{"id","ref","current_directory","remote":{"enabled"}}]}`.
//! - `set-status <key> <value> --workspace <id>` puts a pill on a workspace's
//!   sidebar row (a tool owns its own keys), `set-progress` a progress bar,
//!   `notify` a notification.
//!
//! Everything goes through a [`CmuxRunner`] so tests use fixtures; nothing
//! here may run on the UI thread.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::{debug, info, warn};

const TIMEOUT: Duration = Duration::from_secs(5);

/// Result of one `cmux` call.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CmuxOutput {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

impl CmuxOutput {
    pub fn success(stdout: impl Into<String>) -> Self {
        CmuxOutput { ok: true, stdout: stdout.into(), stderr: String::new() }
    }
    pub fn failure(stderr: impl Into<String>) -> Self {
        CmuxOutput { ok: false, stdout: String::new(), stderr: stderr.into() }
    }

    fn into_result(self) -> Result<String, String> {
        if self.ok {
            Ok(self.stdout)
        } else {
            let msg = self.stderr.trim();
            Err(if msg.is_empty() { "cmux failed".to_string() } else { msg.trim_start_matches("Error: ").to_string() })
        }
    }
}

/// Runs `cmux <args>`. Swapped for a fixture runner in tests.
pub trait CmuxRunner: Send + Sync {
    fn run(&self, args: &[String]) -> CmuxOutput;
}

/// The real `cmux` ([`cli_path`]).
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemCmux;

impl CmuxRunner for SystemCmux {
    fn run(&self, args: &[String]) -> CmuxOutput {
        let Some(cli) = cli_path() else {
            return CmuxOutput::failure("cmux isn't installed");
        };
        let spawned = Command::new(cli)
            .args(args)
            .env("CMUX_QUIET", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();
        let mut child = match spawned {
            Ok(c) => c,
            Err(e) => return CmuxOutput::failure(format!("failed to run cmux: {e}")),
        };
        // Drain both pipes on threads so a long listing can't deadlock on a full pipe.
        let mut out = child.stdout.take();
        let mut err = child.stderr.take();
        let t_out = std::thread::spawn(move || {
            let mut s = String::new();
            if let Some(o) = out.as_mut() {
                let _ = o.read_to_string(&mut s);
            }
            s
        });
        let t_err = std::thread::spawn(move || {
            let mut s = String::new();
            if let Some(e) = err.as_mut() {
                let _ = e.read_to_string(&mut s);
            }
            s
        });
        let deadline = Instant::now() + TIMEOUT;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) if Instant::now() >= deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                Err(_) => break None,
            }
        };
        let stdout = t_out.join().unwrap_or_default();
        let stderr = t_err.join().unwrap_or_default();
        match status {
            Some(s) => CmuxOutput { ok: s.success(), stdout, stderr },
            None => CmuxOutput { ok: false, stdout, stderr: format!("cmux timed out after {}s", TIMEOUT.as_secs()) },
        }
    }
}

/// The `cmux` CLI: the one cmux put in this terminal's environment, else on
/// `$PATH`, else inside the app bundle.
pub fn cli_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("CMUX_BUNDLED_CLI_PATH").map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(p);
    }
    if let Some(p) = std::env::var_os("PATH")
        .and_then(|path| std::env::split_paths(&path).map(|d| d.join("cmux")).find(|p| p.is_file()))
    {
        return Some(p);
    }
    let bundled = Path::new("/Applications/cmux.app/Contents/Resources/bin/cmux");
    (cfg!(target_os = "macos") && bundled.is_file()).then(|| bundled.to_path_buf())
}

/// Running in a cmux terminal (or a process started from one), so the
/// control socket answers.
pub fn inside() -> bool {
    own_workspace().is_some()
}

/// The workspace this process runs in (`CMUX_WORKSPACE_ID`), if inside cmux.
pub fn own_workspace() -> Option<String> {
    std::env::var("CMUX_WORKSPACE_ID").ok().filter(|v| !v.is_empty())
}

/// A cmux workspace (a sidebar row).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    pub id: String,
    /// Working directory of its focused terminal.
    pub directory: Option<String>,
}

fn rpc(r: &dyn CmuxRunner, method: &str, params: &Value) -> Result<Value, String> {
    let out = r.run(&["rpc".into(), method.into(), params.to_string()]).into_result()?;
    serde_json::from_str(&out).map_err(|e| format!("cmux rpc {method}: unexpected output ({e})"))
}

/// Every local workspace in every window (SSH workspaces left out: their
/// directory is on another machine).
pub fn list_workspaces(r: &dyn CmuxRunner) -> Result<Vec<Workspace>, String> {
    let windows = rpc(r, "window.list", &serde_json::json!({}))?;
    let mut out = Vec::new();
    for window in windows["windows"].as_array().into_iter().flatten() {
        let Some(id) = window["id"].as_str() else { continue };
        let listed = rpc(r, "workspace.list", &serde_json::json!({ "window_id": id }))?;
        for w in listed["workspaces"].as_array().into_iter().flatten() {
            if w["remote"]["enabled"].as_bool() == Some(true) {
                continue;
            }
            let Some(id) = w["id"].as_str().or_else(|| w["ref"].as_str()) else { continue };
            let directory = w["current_directory"].as_str().filter(|d| !d.is_empty()).map(str::to_string);
            out.push(Workspace { id: id.to_string(), directory });
        }
    }
    debug!("cmux", "Listed {} workspace(s)", out.len());
    Ok(out)
}

/// The most specific of `roots` that contains `dir` (a worktree nested in
/// another, like `.claude/worktrees/x` in its repo, belongs to itself).
pub fn owner_of<'a>(dir: &str, roots: &[&'a str]) -> Option<&'a str> {
    let dir = Path::new(dir);
    roots.iter().copied().filter(|r| dir.starts_with(r)).max_by_key(|r| r.len())
}

/// The first workspace whose directory belongs to `root` (most specific
/// owner among `roots`, which should include `root`).
pub fn workspace_for<'w>(workspaces: &'w [Workspace], root: &str, roots: &[&str]) -> Option<&'w Workspace> {
    workspaces.iter().find(|w| w.directory.as_deref().and_then(|d| owner_of(d, roots)) == Some(root))
}

pub fn select_workspace(r: &dyn CmuxRunner, id: &str) -> Result<(), String> {
    r.run(&["workspace".into(), "select".into(), id.into()]).into_result().map(drop)
}

/// A new workspace at `cwd` named `name`, its terminal running `command`,
/// switched to (`--focus true`: cmux creates it in the background otherwise).
pub fn create_workspace(r: &dyn CmuxRunner, cwd: &Path, name: &str, command: Option<&str>) -> Result<(), String> {
    let mut args: Vec<String> = vec![
        "workspace".into(),
        "create".into(),
        "--cwd".into(),
        cwd.to_string_lossy().into_owned(),
        "--name".into(),
        name.into(),
        "--focus".into(),
        "true".into(),
    ];
    if let Some(c) = command.filter(|c| !c.trim().is_empty()) {
        args.extend(["--command".into(), c.into()]);
    }
    let result = r.run(&args).into_result().map(drop);
    match &result {
        Ok(()) => info!("cmux", "Created workspace {name} at {} (command: {})", cwd.display(), command.unwrap_or("-")),
        Err(e) => warn!("cmux", "Couldn't create workspace {name} at {}: {e}", cwd.display()),
    }
    result
}

/// Open `path` as a workspace through `cmux <path>`, which needs no socket
/// (and launches cmux when it isn't running).
pub fn open_path(r: &dyn CmuxRunner, path: &Path) -> Result<(), String> {
    let result = r.run(&[path.to_string_lossy().into_owned()]).into_result().map(drop);
    match &result {
        Ok(()) => info!("cmux", "Opened {} in cmux", path.display()),
        Err(e) => warn!("cmux", "Couldn't open {} in cmux: {e}", path.display()),
    }
    result
}

/// A sidebar status pill on a workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pill {
    pub value: String,
    /// SF Symbol name.
    pub icon: Option<&'static str>,
    /// `#rrggbb`.
    pub color: Option<&'static str>,
    /// Higher sorts first.
    pub priority: i32,
}

pub fn set_status(r: &dyn CmuxRunner, workspace: &str, key: &str, pill: &Pill) -> Result<(), String> {
    let mut args: Vec<String> = vec!["set-status".into(), key.into(), pill.value.clone(), "--workspace".into(), workspace.into()];
    if let Some(icon) = pill.icon {
        args.extend(["--icon".into(), icon.into()]);
    }
    if let Some(color) = pill.color {
        args.extend(["--color".into(), color.into()]);
    }
    args.extend(["--priority".into(), pill.priority.to_string()]);
    r.run(&args).into_result().map(drop)
}

pub fn clear_status(r: &dyn CmuxRunner, workspace: &str, key: &str) -> Result<(), String> {
    r.run(&["clear-status".into(), key.into(), "--workspace".into(), workspace.into()]).into_result().map(drop)
}

/// The workspace's progress bar: `value` in 0..=1, or `None` to clear it.
pub fn set_progress(r: &dyn CmuxRunner, workspace: &str, value: Option<(f64, &str)>) -> Result<(), String> {
    let args: Vec<String> = match value {
        Some((v, label)) => vec![
            "set-progress".into(),
            format!("{:.2}", v.clamp(0.0, 1.0)),
            "--label".into(),
            label.into(),
            "--workspace".into(),
            workspace.into(),
        ],
        None => vec!["clear-progress".into(), "--workspace".into(), workspace.into()],
    };
    r.run(&args).into_result().map(drop)
}

/// A cmux notification, on `workspace` or (when `None`) the caller's own.
pub fn notify(r: &dyn CmuxRunner, title: &str, body: &str, workspace: Option<&str>) -> Result<(), String> {
    let mut args: Vec<String> = vec!["notify".into(), "--title".into(), title.into(), "--body".into(), body.into()];
    if let Some(w) = workspace {
        args.extend(["--workspace".into(), w.into()]);
    }
    let result = r.run(&args).into_result().map(drop);
    match &result {
        Ok(()) => info!("cmux", "Notified: {title} — {body}"),
        Err(e) => warn!("cmux", "Couldn't notify ({title}): {e}"),
    }
    result
}

/// `~/.config/cmux/dock.json` (personal) or `<dir>/.cmux/dock.json` (a
/// project's, shared through the repo).
pub fn dock_path(project: Option<&Path>) -> PathBuf {
    match project {
        Some(dir) => dir.join(".cmux").join("dock.json"),
        None => crate::paths::home_dir().join(".config").join("cmux").join("dock.json"),
    }
}

/// A Dock control (a terminal in cmux's right sidebar running `command`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockControl {
    pub id: &'static str,
    pub title: &'static str,
    pub command: String,
}

/// The controls `am cmux dock` adds: the dashboard, and optionally the
/// review page. `am` is the absolute path for the personal Dock (so it
/// doesn't depend on `$PATH`), plain `am` for a project's shared one.
pub fn am_dock_controls(am: &str, reviews: bool) -> Vec<DockControl> {
    let am = crate::launch::sh_quote(am);
    let mut out = vec![DockControl { id: "am", title: "am", command: am.clone() }];
    if reviews {
        out.push(DockControl { id: "am-reviews", title: "Reviews", command: format!("{am} reviews") });
    }
    out
}

/// Whether `dock` holds any of `controls` (by id).
pub fn has_am_controls(dock: &serde_json::Map<String, Value>, controls: &[DockControl]) -> bool {
    dock.get("controls")
        .and_then(Value::as_array)
        .is_some_and(|list| list.iter().any(|c| c["id"].as_str().is_some_and(|id| controls.iter().any(|a| a.id == id))))
}

/// `dock` (a parsed dock.json, `{}` when new) with `controls` added or, by
/// id, replaced, and every other control kept in place. With `remove`, am's
/// controls are taken out instead. `Err` when the file isn't dock-shaped.
pub fn merge_dock(dock: &mut serde_json::Map<String, Value>, controls: &[DockControl], remove: bool) -> Result<(), String> {
    let list = dock.entry("controls").or_insert_with(|| Value::Array(Vec::new()));
    let Value::Array(list) = list else {
        return Err("\"controls\" isn't a list".into());
    };
    let ids: Vec<&str> = controls.iter().map(|c| c.id).collect();
    if remove {
        list.retain(|c| !c["id"].as_str().is_some_and(|id| ids.contains(&id)));
        return Ok(());
    }
    for c in controls {
        let entry = serde_json::json!({ "id": c.id, "title": c.title, "command": c.command });
        match list.iter_mut().find(|e| e["id"].as_str() == Some(c.id)) {
            Some(e) => *e = entry,
            None => list.push(entry),
        }
    }
    Ok(())
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Answers by the first argument (or `rpc <method>`), recording calls.
    #[derive(Default)]
    pub struct FakeCmux {
        pub answers: Vec<(String, CmuxOutput)>,
        pub calls: Mutex<Vec<Vec<String>>>,
    }

    impl FakeCmux {
        pub fn with(mut self, key: &str, out: CmuxOutput) -> Self {
            self.answers.push((key.into(), out));
            self
        }
        pub fn calls(&self) -> Vec<Vec<String>> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl CmuxRunner for FakeCmux {
        fn run(&self, args: &[String]) -> CmuxOutput {
            self.calls.lock().unwrap().push(args.to_vec());
            let key = match args {
                [rpc, method, ..] if rpc == "rpc" => format!("rpc {method}"),
                [first, ..] => first.clone(),
                [] => String::new(),
            };
            self.answers.iter().find(|(k, _)| *k == key).map(|(_, o)| o.clone()).unwrap_or_else(|| CmuxOutput::success(""))
        }
    }

    fn listing() -> FakeCmux {
        FakeCmux::default().with("rpc window.list", CmuxOutput::success(r#"{"windows":[{"id":"W1"}]}"#)).with(
            "rpc workspace.list",
            CmuxOutput::success(
                r#"{"workspaces":[
                    {"id":"A","ref":"workspace:1","current_directory":"/r/repo"},
                    {"id":"B","ref":"workspace:2","current_directory":"/r/repo/.claude/worktrees/feat/src"},
                    {"id":"C","ref":"workspace:3","current_directory":"/home/x","remote":{"enabled":true}},
                    {"ref":"workspace:4"}
                ]}"#,
            ),
        )
    }

    #[test]
    fn lists_local_workspaces_of_every_window() {
        let fake = listing();
        let ws = list_workspaces(&fake).unwrap();
        assert_eq!(
            ws,
            vec![
                Workspace { id: "A".into(), directory: Some("/r/repo".into()) },
                Workspace { id: "B".into(), directory: Some("/r/repo/.claude/worktrees/feat/src".into()) },
                Workspace { id: "workspace:4".into(), directory: None },
            ]
        );
        assert_eq!(fake.calls()[1], vec!["rpc".to_string(), "workspace.list".into(), r#"{"window_id":"W1"}"#.into()]);
    }

    #[test]
    fn a_workspace_belongs_to_its_most_specific_worktree() {
        let ws = list_workspaces(&listing()).unwrap();
        let roots = ["/r/repo", "/r/repo/.claude/worktrees/feat", "/r/other"];
        assert_eq!(workspace_for(&ws, "/r/repo", &roots).map(|w| w.id.as_str()), Some("A"));
        assert_eq!(workspace_for(&ws, "/r/repo/.claude/worktrees/feat", &roots).map(|w| w.id.as_str()), Some("B"));
        assert_eq!(workspace_for(&ws, "/r/other", &roots), None);
        // Path components, not string prefixes.
        assert_eq!(owner_of("/r/repo-2/x", &roots), None);
    }

    #[test]
    fn socket_errors_come_back_readable() {
        let fake = FakeCmux::default().with("rpc window.list", CmuxOutput::failure("Error: Failed to write to socket (Broken pipe, errno 32)\n"));
        assert_eq!(list_workspaces(&fake).unwrap_err(), "Failed to write to socket (Broken pipe, errno 32)");
    }

    #[test]
    fn dock_controls_merge_by_id_and_keep_the_rest() {
        let mut dock: serde_json::Map<String, Value> =
            serde_json::from_str(r#"{"controls":[{"id":"git","title":"Git","command":"lazygit"},{"id":"am","title":"old","command":"am"}]}"#).unwrap();
        merge_dock(&mut dock, &am_dock_controls("/opt/am bin/am", true), false).unwrap();
        let ids: Vec<&str> = dock["controls"].as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["git", "am", "am-reviews"]);
        assert_eq!(dock["controls"][1]["command"], "'/opt/am bin/am'");
        assert_eq!(dock["controls"][2]["command"], "'/opt/am bin/am' reviews");
        assert!(has_am_controls(&dock, &am_dock_controls("am", true)));
        merge_dock(&mut dock, &am_dock_controls("am", true), true).unwrap();
        assert_eq!(dock["controls"].as_array().unwrap().len(), 1);
        assert!(!has_am_controls(&dock, &am_dock_controls("am", true)));
        assert!(!has_am_controls(&serde_json::Map::new(), &am_dock_controls("am", true)));

        let mut fresh = serde_json::Map::new();
        merge_dock(&mut fresh, &am_dock_controls("am", false), false).unwrap();
        assert_eq!(Value::Object(fresh), serde_json::json!({"controls":[{"id":"am","title":"am","command":"am"}]}));

        let mut bad: serde_json::Map<String, Value> = serde_json::from_str(r#"{"controls":{}}"#).unwrap();
        assert!(merge_dock(&mut bad, &am_dock_controls("am", false), false).is_err());
    }

    #[test]
    fn command_lines() {
        let fake = FakeCmux::default();
        create_workspace(&fake, Path::new("/r/w"), "feat", Some("claude --resume abc")).unwrap();
        create_workspace(&fake, Path::new("/r/w"), "feat", None).unwrap();
        set_status(&fake, "A", "am_ci", &Pill { value: "✓ CI".into(), icon: None, color: Some("#34c759"), priority: 5 }).unwrap();
        set_progress(&fake, "A", Some((1.7, "ctx 42%"))).unwrap();
        set_progress(&fake, "A", None).unwrap();
        let calls: Vec<String> = fake.calls().iter().map(|c| c.join(" ")).collect();
        assert_eq!(
            calls,
            [
                "workspace create --cwd /r/w --name feat --focus true --command claude --resume abc",
                "workspace create --cwd /r/w --name feat --focus true",
                "set-status am_ci ✓ CI --workspace A --color #34c759 --priority 5",
                "set-progress 1.00 --label ctx 42% --workspace A",
                "clear-progress --workspace A",
            ]
        );
    }
}
