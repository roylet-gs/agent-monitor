//! Terminal backends: open a worktree in a tab / split / window of the
//! terminal am runs in, and focus a pane or tab already sitting there.
//!
//! The backend comes from the `terminalBackend` setting, else the
//! environment: inside tmux (`$TMUX`) tmux wins, so new panes land in the
//! session you're in; otherwise `$TERM_PROGRAM` picks the app. Every command
//! is built by a pure function so the argv / AppleScript is unit-tested; only
//! [`open`] and [`focus`] spawn anything.
//!
//! Like `launch`, no keystroke injection: Ghostty gets its command through
//! the surface configuration's `initial input`, iTerm2 and Terminal.app via
//! AppleScript `command` / `do script`.

use std::path::Path;
use std::process::{Command, Stdio};

use crate::{debug, info, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Tmux,
    Ghostty,
    ITerm2,
    Warp,
    TerminalApp,
    /// Linux fallback (`x-terminal-emulator`).
    XTerminalEmulator,
}

impl Backend {
    pub fn name(self) -> &'static str {
        match self {
            Backend::Tmux => "tmux",
            Backend::Ghostty => "Ghostty",
            Backend::ITerm2 => "iTerm2",
            Backend::Warp => "Warp",
            Backend::TerminalApp => "Terminal",
            Backend::XTerminalEmulator => "x-terminal-emulator",
        }
    }

    /// The `terminalBackend` setting value; `None` for `auto` / unknown.
    pub fn from_setting(s: &str) -> Option<Backend> {
        Some(match s {
            "tmux" => Backend::Tmux,
            "ghostty" => Backend::Ghostty,
            "iterm2" => Backend::ITerm2,
            "warp" => Backend::Warp,
            "terminal" => Backend::TerminalApp,
            "x-terminal-emulator" => Backend::XTerminalEmulator,
            _ => return None,
        })
    }
}

/// Setting values offered in the settings editor, `auto` first.
pub const BACKEND_CHOICES: &[&str] = &["auto", "tmux", "ghostty", "iterm2", "warp", "terminal", "x-terminal-emulator"];
pub const PLACEMENT_CHOICES: &[&str] = &["tab", "split", "window"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    Tab,
    Split,
    Window,
}

impl Placement {
    pub fn from_setting(s: &str) -> Placement {
        match s {
            "split" => Placement::Split,
            "window" => Placement::Window,
            _ => Placement::Tab,
        }
    }
}

/// What detection looks at, so it can be tested without touching the process env.
#[derive(Debug, Clone, Default)]
pub struct Env {
    pub tmux: bool,
    pub term_program: Option<String>,
    pub macos: bool,
}

impl Env {
    pub fn current() -> Env {
        Env {
            tmux: std::env::var_os("TMUX").is_some_and(|v| !v.is_empty()),
            term_program: std::env::var("TERM_PROGRAM").ok(),
            macos: cfg!(target_os = "macos"),
        }
    }
}

pub fn detect_from(setting: &str, env: &Env) -> Backend {
    if let Some(b) = Backend::from_setting(setting) {
        return b;
    }
    if env.tmux {
        return Backend::Tmux;
    }
    match env.term_program.as_deref() {
        Some("ghostty") => Backend::Ghostty,
        Some("iTerm.app") if env.macos => Backend::ITerm2,
        Some("WarpTerminal") if env.macos => Backend::Warp,
        // Inside an editor's integrated terminal (TERM_PROGRAM=vscode) there
        // is no app to drive, so use the platform's default terminal.
        _ if env.macos => Backend::TerminalApp,
        _ => Backend::XTerminalEmulator,
    }
}

pub fn detect(setting: &str) -> Backend {
    detect_from(setting, &Env::current())
}

pub fn sh_quote(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "/._-+:@".contains(c)) {
        return s.to_string();
    }
    format!("'{}'", s.replace('\'', "'\\''"))
}

pub fn applescript_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn osascript(script: String) -> Command {
    let mut c = Command::new("osascript");
    c.arg("-e").arg(script);
    c
}

/// The command(s) that open `dir` (running `cmd`, then staying in a shell).
/// More than one means "try in order until one succeeds" (Ghostty's
/// AppleScript needs 1.3+; older versions fall back to a new instance).
pub fn open_commands(backend: Backend, dir: &str, cmd: Option<&str>, placement: Placement, name: Option<&str>) -> Vec<Command> {
    let cd_cmd = match cmd {
        Some(c) => format!("cd {} && {c}", sh_quote(dir)),
        None => format!("cd {}", sh_quote(dir)),
    };
    match backend {
        Backend::Tmux => {
            let mut c = Command::new("tmux");
            match placement {
                Placement::Split => {
                    c.args(["split-window", "-h", "-c", dir]);
                }
                Placement::Tab | Placement::Window => {
                    c.args(["new-window", "-c", dir]);
                    if let Some(n) = name {
                        c.args(["-n", n]);
                    }
                }
            }
            // tmux runs it via `default-shell -c`; exec a shell after so the
            // pane stays when claude exits.
            if let Some(cmd) = cmd {
                c.arg(format!("{cmd}; exec \"${{SHELL:-/bin/sh}}\""));
            }
            vec![c]
        }
        Backend::Ghostty if cfg!(target_os = "macos") => {
            let input = cmd.map(|c| format!("\n set initial input of cfg to {}", applescript_string(&format!("{c}\n")))).unwrap_or_default();
            let place = match placement {
                Placement::Tab => "new tab in front window with configuration cfg",
                Placement::Split => "split (focused terminal of selected tab of front window) direction right with configuration cfg",
                Placement::Window => "new window with configuration cfg",
            };
            let script = format!(
                "tell application \"Ghostty\"\n activate\n set cfg to new surface configuration\n set initial working directory of cfg to {}{input}\n if (count of windows) is 0 then\n  new window with configuration cfg\n else\n  {place}\n end if\nend tell",
                applescript_string(dir)
            );
            let mut fallback = Command::new("open");
            fallback.args(["-na", "Ghostty", "--args"]).arg(format!("--working-directory={dir}"));
            if let Some(cmd) = cmd {
                fallback.arg("-e").args(["/bin/zsh", "-lc", &format!("{cmd}; exec $SHELL")]);
            }
            vec![osascript(script), fallback]
        }
        Backend::Ghostty => {
            let mut c = Command::new("ghostty");
            c.arg("+new-window").arg(format!("--working-directory={dir}"));
            if let Some(cmd) = cmd {
                c.args(["-e", "sh", "-c", &format!("{cmd}; exec \"${{SHELL:-/bin/sh}}\"")]);
            }
            vec![c]
        }
        Backend::ITerm2 => {
            let command = applescript_string(&format!("/bin/zsh -lc {}", sh_quote(&format!("{cd_cmd}; exec $SHELL"))));
            let place = match placement {
                Placement::Tab => format!("tell current window to create tab with default profile command {command}"),
                Placement::Split => format!("tell current session of current window to split vertically with default profile command {command}"),
                Placement::Window => format!("create window with default profile command {command}"),
            };
            vec![osascript(format!(
                // iTerm2's AppleScript name is "iTerm" ("iTerm2" doesn't resolve).
                "tell application \"iTerm\"\n activate\n if (count of windows) > 0 then\n  {place}\n else\n  create window with default profile command {command}\n end if\nend tell"
            ))]
        }
        // Warp can't run a command from outside (no API; URIs only open a
        // path), so with a command it goes to Terminal.app.
        Backend::Warp if cmd.is_none() => {
            let mut c = Command::new("open");
            c.args(["-a", "Warp", dir]);
            vec![c]
        }
        Backend::Warp | Backend::TerminalApp => {
            vec![osascript(format!("tell application \"Terminal\"\n activate\n do script {}\nend tell", applescript_string(&cd_cmd)))]
        }
        Backend::XTerminalEmulator => {
            let mut c = Command::new("x-terminal-emulator");
            c.arg(format!("--working-directory={dir}"));
            if let Some(cmd) = cmd {
                c.args(["-e", "sh", "-c", cmd]);
            }
            vec![c]
        }
    }
}

fn run(cmd: &mut Command) -> Result<String, String> {
    let out = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).lines().next().unwrap_or("failed").to_string())
    }
}

/// Open a new tab / split / window at `path`, optionally running `command`.
pub fn open(backend: Backend, path: &Path, command: Option<&str>, placement: Placement) -> Result<(), String> {
    let dir = path.to_string_lossy();
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
    let mut last = Err("no command".to_string());
    for mut c in open_commands(backend, &dir, command, placement, name.as_deref()) {
        last = run(&mut c).map(|_| ());
        match &last {
            Ok(()) => break,
            Err(e) => debug!("terminal", "{} open attempt {:?} failed: {e}", backend.name(), c.get_program()),
        }
    }
    match &last {
        Ok(()) => info!("terminal", "Opened {} {placement:?} at {dir} (command: {})", backend.name(), command.unwrap_or("-")),
        Err(e) => warn!("terminal", "Failed to open {} at {dir}: {e}", backend.name()),
    }
    last.map_err(|e| format!("Couldn't open {}: {e}", backend.name()))
}

/// A pane / terminal surface and the directory its shell is in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Surface {
    pub id: String,
    pub cwd: String,
}

/// `id<TAB>cwd` lines (tmux `list-panes` / the Ghostty listing script).
pub fn parse_surfaces(out: &str) -> Vec<Surface> {
    out.lines()
        .filter_map(|l| {
            let (id, cwd) = l.split_once('\t')?;
            let cwd = cwd.trim().strip_prefix("file://").unwrap_or(cwd.trim());
            let cwd = if cwd.len() > 1 { cwd.trim_end_matches('/') } else { cwd };
            (!id.is_empty() && !cwd.is_empty()).then(|| Surface { id: id.to_string(), cwd: cwd.to_string() })
        })
        .collect()
}

/// The surface to focus for worktree `root`: one whose cwd is the root or
/// below it, but not inside a worktree nested under it (am keeps worktrees at
/// `<repo>/.claude/worktrees/…`, inside the main checkout). Closest to the
/// root wins.
pub fn pick_surface<'a>(surfaces: &'a [Surface], root: &str) -> Option<&'a Surface> {
    let root = root.trim_end_matches('/');
    surfaces
        .iter()
        .filter(|s| match s.cwd.strip_prefix(root) {
            Some("") => true,
            Some(rest) => rest.starts_with('/') && !rest.contains("/.claude/worktrees/"),
            None => false,
        })
        .min_by_key(|s| s.cwd.len())
}

fn list_command(backend: Backend) -> Option<Command> {
    match backend {
        Backend::Tmux => {
            let mut c = Command::new("tmux");
            c.args(["list-panes", "-a", "-F", "#{pane_id}\t#{pane_current_path}"]);
            Some(c)
        }
        Backend::Ghostty if cfg!(target_os = "macos") => Some(osascript(
            // `is running` first: a `tell` would launch Ghostty.
            "if application \"Ghostty\" is running then\n tell application \"Ghostty\"\n  set out to \"\"\n  repeat with t in terminals\n   set out to out & (id of t) & tab & (working directory of t) & linefeed\n  end repeat\n  return out\n end tell\nend if".into(),
        )),
        _ => None,
    }
}

/// The command that focuses a surface picked from the listing.
pub fn focus_surface_command(backend: Backend, id: &str) -> Option<Command> {
    match backend {
        Backend::Tmux => {
            // switch-client moves the attached client to the pane's session
            // and window; select-pane picks the pane inside it.
            let mut c = Command::new("sh");
            c.args(["-c", "tmux switch-client -t \"$1\" 2>/dev/null; tmux select-window -t \"$1\" && tmux select-pane -t \"$1\"", "sh", id]);
            Some(c)
        }
        Backend::Ghostty => Some(osascript(format!(
            "tell application \"Ghostty\"\n repeat with t in terminals\n  if id of t is {} then\n   focus t\n   activate\n   return\n  end if\n end repeat\nend tell",
            applescript_string(id)
        ))),
        _ => None,
    }
}

fn activate_command(backend: Backend) -> Option<Command> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let app = match backend {
        Backend::ITerm2 => "iTerm",
        Backend::Ghostty | Backend::Warp | Backend::TerminalApp => backend.name(),
        Backend::Tmux | Backend::XTerminalEmulator => return None,
    };
    Some(osascript(format!("tell application {} to activate", applescript_string(app))))
}

/// Bring forward a terminal sitting at `root`. `Ok(true)` when a specific
/// pane / tab was focused; `Ok(false)` when none was found and only the app
/// could be activated (or nothing at all, for tmux), so the caller may
/// prefer to open a new one.
pub fn focus(backend: Backend, root: &Path) -> Result<bool, String> {
    let root_s = root.to_string_lossy();
    if let Some(mut list) = list_command(backend) {
        match run(&mut list) {
            Ok(out) => {
                let surfaces = parse_surfaces(&out);
                if let Some(s) = pick_surface(&surfaces, &root_s)
                    && let Some(mut c) = focus_surface_command(backend, &s.id)
                {
                    run(&mut c).map_err(|e| format!("Couldn't focus {}: {e}", backend.name()))?;
                    info!("terminal", "Focused {} surface {} at {}", backend.name(), s.id, s.cwd);
                    return Ok(true);
                }
                debug!("terminal", "No {} surface at {root_s} among {} listed", backend.name(), surfaces.len());
            }
            Err(e) => debug!("terminal", "Listing {} surfaces failed: {e}", backend.name()),
        }
    }
    if let Some(mut c) = activate_command(backend) {
        run(&mut c).map_err(|e| format!("Couldn't activate {}: {e}", backend.name()))?;
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(tmux: bool, tp: Option<&str>, macos: bool) -> Env {
        Env { tmux, term_program: tp.map(str::to_owned), macos }
    }

    fn argv(c: &Command) -> Vec<String> {
        std::iter::once(c.get_program()).chain(c.get_args()).map(|a| a.to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn detection_order() {
        assert_eq!(detect_from("auto", &env(true, Some("ghostty"), true)), Backend::Tmux, "tmux wins inside tmux");
        assert_eq!(detect_from("auto", &env(false, Some("ghostty"), true)), Backend::Ghostty);
        assert_eq!(detect_from("auto", &env(false, Some("ghostty"), false)), Backend::Ghostty);
        assert_eq!(detect_from("auto", &env(false, Some("iTerm.app"), true)), Backend::ITerm2);
        assert_eq!(detect_from("auto", &env(false, Some("WarpTerminal"), true)), Backend::Warp);
        assert_eq!(detect_from("auto", &env(false, Some("vscode"), true)), Backend::TerminalApp);
        assert_eq!(detect_from("auto", &env(false, None, false)), Backend::XTerminalEmulator);
        assert_eq!(detect_from("ghostty", &env(true, None, true)), Backend::Ghostty, "explicit setting wins");
        assert_eq!(detect_from("bogus", &env(false, Some("ghostty"), true)), Backend::Ghostty);
    }

    #[test]
    fn tmux_commands() {
        let c = &open_commands(Backend::Tmux, "/w/a b", Some("claude --resume x"), Placement::Tab, Some("a b"))[0];
        assert_eq!(argv(c), ["tmux", "new-window", "-c", "/w/a b", "-n", "a b", "claude --resume x; exec \"${SHELL:-/bin/sh}\""]);
        let c = &open_commands(Backend::Tmux, "/w/a", None, Placement::Split, None)[0];
        assert_eq!(argv(c), ["tmux", "split-window", "-h", "-c", "/w/a"]);
        let f = focus_surface_command(Backend::Tmux, "%3").unwrap();
        assert_eq!(argv(&f).last().unwrap(), "%3");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn ghostty_applescript_then_fallback() {
        let cmds = open_commands(Backend::Ghostty, "/w/\"q\"", Some("claude -c"), Placement::Split, None);
        assert_eq!(cmds.len(), 2);
        let script = argv(&cmds[0])[2].clone();
        assert!(script.contains("set initial working directory of cfg to \"/w/\\\"q\\\"\""), "{script}");
        assert!(script.contains("set initial input of cfg to \"claude -c\n\""), "{script}");
        assert!(script.contains("split (focused terminal of selected tab of front window) direction right"), "{script}");
        assert_eq!(argv(&cmds[1])[..4], ["open", "-na", "Ghostty", "--args"]);
        let tab = open_commands(Backend::Ghostty, "/w", None, Placement::Tab, None);
        let script = argv(&tab[0])[2].clone();
        assert!(script.contains("new tab in front window") && !script.contains("initial input"), "{script}");
    }

    #[test]
    fn iterm_and_terminal_app() {
        let c = &open_commands(Backend::ITerm2, "/w", Some("claude"), Placement::Split, None)[0];
        assert!(argv(c)[2].starts_with("tell application \"iTerm\""));
        assert!(argv(c)[2].contains("split vertically with default profile command \"/bin/zsh -lc 'cd /w && claude; exec $SHELL'\""));
        let c = &open_commands(Backend::Warp, "/w", Some("claude"), Placement::Tab, None)[0];
        assert!(argv(c)[2].contains("tell application \"Terminal\""), "Warp with a command → Terminal.app");
        let c = &open_commands(Backend::Warp, "/w", None, Placement::Tab, None)[0];
        assert_eq!(argv(c), ["open", "-a", "Warp", "/w"]);
    }

    #[test]
    fn surfaces_parse_and_pick() {
        let s = parse_surfaces("%1\t/repo\n%2\t/repo/.claude/worktrees/feat\n%3\t/repo/src/\n%4\tfile:///other\nbad line\n");
        assert_eq!(s.len(), 4);
        assert_eq!(s[2].cwd, "/repo/src");
        assert_eq!(s[3].cwd, "/other");
        assert_eq!(pick_surface(&s, "/repo").map(|s| s.id.as_str()), Some("%1"));
        assert_eq!(pick_surface(&s, "/repo/.claude/worktrees/feat").map(|s| s.id.as_str()), Some("%2"));
        assert_eq!(pick_surface(&s, "/rep"), None, "component-wise prefix");
        let only_nested = parse_surfaces("%2\t/repo/.claude/worktrees/feat\n");
        assert_eq!(pick_surface(&only_nested, "/repo"), None, "a nested worktree's pane isn't the main checkout's");
        let sub = parse_surfaces("%9\t/repo/src\n");
        assert_eq!(pick_surface(&sub, "/repo/").map(|s| s.id.as_str()), Some("%9"));
    }

    #[test]
    fn quoting() {
        assert_eq!(sh_quote("/plain/path-1.2"), "/plain/path-1.2");
        assert_eq!(sh_quote("it's"), "'it'\\''s'");
        assert_eq!(applescript_string("a \"b\" \\c"), "\"a \\\"b\\\" \\\\c\"");
    }
}
