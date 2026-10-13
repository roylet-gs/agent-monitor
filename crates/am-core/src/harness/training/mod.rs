//! Training mode: let an unsupported harness write its own am plugin.
//!
//! [`prepare`] makes a workspace `<app dir>/training/<slug>/` (a tiny git repo,
//! so the session shows up under "Other sessions" while it trains), renders the
//! create or edit prompt into `TRAINING.md` there, and works out the command
//! that starts the harness. [`start`] copies a one-line bootstrap ("read
//! TRAINING.md…") to the clipboard and opens a terminal running it. A
//! one-liner is pasted rather than the whole prompt because a multi-line paste
//! can submit on its first newline in a TUI harness. When a plugin knows how to
//! take an initial prompt (`training.promptArgs`) no paste is needed.
//!
//! The prompts themselves are `create.md` and `edit.md` next to this file.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::manifest::{CANONICAL_EVENTS, PluginStatus};
use super::{BUILTIN, HarnessKind, issues, lifecycle};
use crate::launch::{self, sh_quote};
use crate::status::CANONICAL_TOOLS;
use crate::{info, paths, warn};

const CREATE: &str = include_str!("create.md");
const EDIT: &str = include_str!("edit.md");

pub const BOOTSTRAP: &str = "Read ./TRAINING.md in this directory and follow it exactly.";

/// The integration shapes a manifest can describe today.
pub const SUPPORTED_INTEGRATIONS: &str = "shell-command hooks in a JSON settings file shaped like Claude Code's \
(`{\"hooks\": {\"<Event>\": [{\"matcher\": \"*\", \"hooks\": [{\"type\": \"command\", \"command\": \"…\"}]}]}}`), \
with the JSON payload on stdin";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    /// Train a new harness plugin for the harness started with `command` (as typed: program + args).
    Create { command: String },
    /// Improve plugin `id`.
    Edit { id: String },
}

#[derive(Debug, Clone)]
pub struct Prepared {
    pub workspace: PathBuf,
    pub prompt_path: PathBuf,
    pub prompt: String,
    /// What to run in the new terminal (inside `workspace`).
    pub command: String,
    /// The bootstrap line must be pasted (the harness can't take a prompt argument).
    pub needs_paste: bool,
    /// Editing turned a published plugin back into a draft.
    pub reverted_to_draft: bool,
}

/// `am` to put in the prompt: the one next to this binary, else `$PATH`'s.
pub fn am_binary() -> String {
    let sibling = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("am"))).filter(|p| p.is_file());
    sibling.map(|p| p.display().to_string()).unwrap_or_else(|| "am".into())
}

/// A workspace name from a typed command: the program's base name as an id.
pub fn slug(command: &str) -> String {
    let program = command.split_whitespace().next().unwrap_or("harness");
    let base = program.rsplit('/').next().unwrap_or(program).to_ascii_lowercase();
    let s: String = base.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let s = s.trim_matches('-').chars().take(32).collect::<String>();
    if s.is_empty() { "harness".into() } else { s }
}

/// Fill `{{key}}` placeholders.
pub fn render(template: &str, vars: &[(&str, String)]) -> String {
    let mut out = template.to_owned();
    for (k, v) in vars {
        out = out.replace(&format!("{{{{{k}}}}}"), v);
    }
    out
}

fn list(items: &[&str]) -> String {
    items.iter().map(|i| format!("`{i}`")).collect::<Vec<_>>().join(", ")
}

/// The rendered prompt for `mode`; `workspace` and `command` as [`prepare`] sets them.
pub fn prompt(mode: &Mode, workspace: &Path, command: &str) -> String {
    let app_dir = paths::app_dir();
    let (template, id) = match mode {
        Mode::Create { .. } => (CREATE, "<id>".to_owned()),
        Mode::Edit { id } => (EDIT, id.clone()),
    };
    let examples: Vec<&str> = BUILTIN.iter().map(|k| k.id()).collect();
    render(
        template,
        &[
            ("am", am_binary()),
            ("app_dir", app_dir.display().to_string()),
            ("workspace", workspace.display().to_string()),
            ("command", command.to_owned()),
            ("id", id.clone()),
            ("manifest_path", lifecycle::manifest_path(&id).display().to_string()),
            ("capture_dir", paths::plugin_capture_dir(&id).display().to_string()),
            ("formats", SUPPORTED_INTEGRATIONS.to_owned()),
            ("canonical_events", list(&CANONICAL_EVENTS)),
            ("canonical_tools", list(&CANONICAL_TOOLS)),
            ("example_ids", list(&examples)),
            ("issue_kinds", list(&issues::ALL_KINDS)),
        ],
    )
}

fn git(dir: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .args(["-c", "user.name=agent-monitor", "-c", "user.email=agent-monitor@localhost"])
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// A small git repo with files the harness can read and edit during probes.
fn ensure_workspace(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("can't create {}: {e}", dir.display()))?;
    if dir.join(".git").exists() {
        return Ok(());
    }
    let readme = "# am training workspace\n\nA scratch repo for teaching agent-monitor a new harness. \
Probe sessions may read this file, list the directory and append to NOTES.txt.\n";
    let write = |name: &str, text: &str| std::fs::write(dir.join(name), text).map_err(|e| e.to_string());
    write("README.md", readme)?;
    write("NOTES.txt", "Probe notes:\n")?;
    write(".gitignore", "TRAINING.md\nmanifest.json\nFINDINGS.md\n")?;
    if !(git(dir, &["init", "-q"]) && git(dir, &["add", "."]) && git(dir, &["commit", "-qm", "training workspace"])) {
        warn!("harness", "git init failed in {}; hooks may not find the workspace", dir.display());
    }
    Ok(())
}

/// The command that starts plugin `kind`'s harness, with `prompt_args`
/// carrying the bootstrap line when the plugin has them.
fn start_command(kind: HarnessKind) -> (String, bool) {
    let p = kind.provider();
    let args = p.manifest().and_then(|m| m.training.as_ref()).map(|t| t.prompt_args.clone()).unwrap_or_default();
    let mut cmd = p.program().to_owned();
    for a in &args {
        cmd.push(' ');
        cmd.push_str(&sh_quote(&a.replace("{prompt}", BOOTSTRAP)));
    }
    let carries_prompt = args.iter().any(|a| a.contains("{prompt}"));
    (cmd, !carries_prompt)
}

/// Set up the workspace and `TRAINING.md` for `mode`. Editing a published
/// plugin turns it back into a draft first, so its events are captured again.
pub fn prepare(mode: &Mode) -> Result<Prepared, String> {
    let (slug, command, needs_paste, reverted) = match mode {
        Mode::Create { command } => {
            let command = command.trim();
            if command.is_empty() {
                return Err("type the command that starts the harness".into());
            }
            (slug(command), command.to_owned(), true, false)
        }
        Mode::Edit { id } => {
            let kind = HarnessKind::get(id).ok_or_else(|| format!("no harness \"{id}\""))?;
            if kind.is_builtin() {
                return Err(format!("{} is built in; only manifest plugins can be retrained", kind.provider().name()));
            }
            let reverted = !kind.is_draft();
            if reverted {
                lifecycle::set_status(id, PluginStatus::Draft)?;
            }
            let (command, needs_paste) = start_command(HarnessKind::get(id).unwrap_or(kind));
            (id.clone(), command, needs_paste, reverted)
        }
    };
    let workspace = paths::training_dir().join(&slug);
    ensure_workspace(&workspace)?;
    let prompt = prompt(mode, &workspace, &command);
    let prompt_path = workspace.join("TRAINING.md");
    std::fs::write(&prompt_path, &prompt).map_err(|e| format!("can't write {}: {e}", prompt_path.display()))?;
    info!("harness", "Prepared training for {slug} in {}", workspace.display());
    Ok(Prepared { workspace, prompt_path, prompt, command, needs_paste, reverted_to_draft: reverted })
}

/// Copy the bootstrap line and open a terminal running the harness in the
/// workspace. Returns what the user should do next.
pub fn start(p: &Prepared) -> Result<String, String> {
    let copied = p.needs_paste && launch::copy_to_clipboard(BOOTSTRAP);
    let opened = launch::open_terminal(&p.workspace, Some(&p.command));
    let draft = if p.reverted_to_draft { " (back in draft while you edit)" } else { "" };
    match (opened, p.needs_paste, copied) {
        (Ok(()), false, _) => Ok(format!("Training started in a new terminal{draft}")),
        (Ok(()), true, true) => Ok(format!("Prompt copied — paste it into {} and press Enter{draft}", p.command)),
        (Ok(()), true, false) => Ok(format!("In {}, type: {BOOTSTRAP}{draft}", p.command)),
        (Err(e), _, _) => Err(format!(
            "Couldn't open a terminal ({e}). Run `cd {} && {}`, then: {BOOTSTRAP}",
            sh_quote(&p.workspace.display().to_string()),
            p.command
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_come_from_the_program_name() {
        assert_eq!(slug("cursor-agent --model x"), "cursor-agent");
        assert_eq!(slug("/opt/bin/Hermes"), "hermes");
        assert_eq!(slug("  "), "harness");
    }

    #[test]
    fn every_placeholder_is_filled() {
        let ws = Path::new("/tmp/ws");
        for mode in [Mode::Create { command: "mycli".into() }, Mode::Edit { id: "mycli".into() }] {
            let text = prompt(&mode, ws, "mycli");
            assert!(!text.contains("{{"), "unfilled placeholder in {mode:?}: {}", &text[text.find("{{").unwrap()..][..40]);
            assert!(text.contains("/tmp/ws"));
            assert!(text.contains("harness add ./manifest.json"));
        }
        assert!(prompt(&Mode::Create { command: "mycli".into() }, ws, "mycli").contains("`PreToolUse`"));
        assert!(prompt(&Mode::Edit { id: "mycli".into() }, ws, "mycli").contains("hook-captures/mycli"));
    }

    #[test]
    fn create_prepares_a_git_workspace_with_the_prompt() {
        let _g = crate::test_env::lock();
        let tmp = tempfile::tempdir().unwrap();
        let _env = crate::test_env::data_dir(tmp.path());
        let p = prepare(&Mode::Create { command: "toycli --flag".into() }).unwrap();
        assert_eq!(p.workspace, tmp.path().join("training/toycli"));
        assert!(p.workspace.join(".git").exists() || Command::new("git").arg("--version").output().is_err());
        assert!(p.workspace.join("README.md").exists());
        assert_eq!(std::fs::read_to_string(&p.prompt_path).unwrap(), p.prompt);
        assert_eq!(p.command, "toycli --flag");
        assert!(p.needs_paste);
        // Idempotent: a second run reuses the workspace.
        prepare(&Mode::Create { command: "toycli".into() }).unwrap();
        assert!(prepare(&Mode::Create { command: " ".into() }).is_err());
        assert!(prepare(&Mode::Edit { id: "claude".into() }).unwrap_err().contains("built in"));
    }

    #[test]
    fn editing_a_published_plugin_reverts_it_to_draft_and_uses_prompt_args() {
        let _g = crate::test_env::lock();
        let tmp = tempfile::tempdir().unwrap();
        let _env = crate::test_env::data_dir(tmp.path());
        let src = tmp.path().join("m.json");
        std::fs::write(
            &src,
            format!(
                r#"{{ "id": "toycli", "name": "Toy", "program": "toycli",
                     "hooks": {{ "settingsPath": "{}", "projectDirVar": "TOY_DIR" }},
                     "events": {{ "Done": "Stop" }}, "status": "published",
                     "training": {{ "promptArgs": ["-i", "{{prompt}}"] }} }}"#,
                tmp.path().join("s.json").display()
            ),
        )
        .unwrap();
        lifecycle::add(&src, &lifecycle::AddOptions { replace: false, keep_status: true, install: false }).unwrap();
        assert!(!HarnessKind::get("toycli").unwrap().is_draft());
        let p = prepare(&Mode::Edit { id: "toycli".into() }).unwrap();
        assert!(p.reverted_to_draft);
        assert!(HarnessKind::get("toycli").unwrap().is_draft());
        assert!(!p.needs_paste);
        assert_eq!(p.command, format!("toycli -i {}", sh_quote(BOOTSTRAP)));
        assert!(p.prompt.contains("Improve the am harness plugin `toycli`"));
    }
}
