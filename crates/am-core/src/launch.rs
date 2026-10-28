//! Opening a worktree: IDE / terminal launch, agent session discovery and the
//! "resume last session" decision. Ported from `src/lib/ide-launcher.ts` and
//! `discoverWorktreeSessions` (`src/lib/claude-session.ts`).
//!
//! Deliberately no keystroke injection into terminals or editors: it needs
//! macOS Accessibility permission and fails silently (osascript error 1002)
//! without it. Editors get the resume command via the clipboard instead.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::SystemTime;

use crate::harness::{self, HarnessKind};
use crate::{info, warn};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub id: String,
    /// Directory the session was started in (the worktree root or a subdir).
    pub cwd: String,
    pub modified: SystemTime,
    pub agent: HarnessKind,
}

/// Every session of every enabled agent started at `root` or any
/// subdirectory, newest first.
pub fn discover_sessions(root: &Path) -> Vec<Session> {
    let enabled = harness::enabled(&crate::settings::load());
    let mut sessions: Vec<Session> = enabled.iter().flat_map(|k| k.provider().discover_sessions(root)).collect();
    sessions.sort_by_key(|s| std::cmp::Reverse(s.modified));
    sessions
}

/// What `Enter` opens a worktree in (the `ide` setting).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdeChoice {
    Vscode,
    Cursor,
    Zed,
    Windsurf,
    Terminal,
    /// The `ideCommand` setting.
    Custom,
}

impl IdeChoice {
    /// Every `ide` value with its label, in the order the settings editor,
    /// setup wizard and CLI offer them.
    pub const ALL: [(&'static str, &'static str); 6] = [
        ("vscode", "VS Code"),
        ("cursor", "Cursor"),
        ("zed", "Zed"),
        ("windsurf", "Windsurf"),
        ("terminal", "Terminal"),
        ("custom", "Custom command"),
    ];
    pub const IDS: [&'static str; 6] = ["vscode", "cursor", "zed", "windsurf", "terminal", "custom"];

    /// Unknown values fall back to VS Code.
    pub fn from_setting(s: &str) -> IdeChoice {
        match s {
            "cursor" => IdeChoice::Cursor,
            "zed" => IdeChoice::Zed,
            "windsurf" => IdeChoice::Windsurf,
            "terminal" => IdeChoice::Terminal,
            "custom" => IdeChoice::Custom,
            _ => IdeChoice::Vscode,
        }
    }

    /// Shell CLI and macOS app name of a built-in editor.
    fn editor(self) -> Option<(&'static str, &'static str)> {
        match self {
            IdeChoice::Vscode => Some(("code", "Visual Studio Code")),
            IdeChoice::Cursor => Some(("cursor", "Cursor")),
            IdeChoice::Zed => Some(("zed", "Zed")),
            IdeChoice::Windsurf => Some(("windsurf", "Windsurf")),
            IdeChoice::Terminal | IdeChoice::Custom => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Resume {
    /// Which agent CLI to start.
    pub agent: HarnessKind,
    pub session_id: Option<String>,
    /// No exact id known, but the worktree has had a session: `claude -c` /
    /// `gemini --resume latest`.
    pub continue_session: bool,
    /// Session start dir when it is a strict subdirectory of the worktree.
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenAction {
    /// Just open the IDE/terminal.
    Plain,
    /// Terminal mode: the new terminal runs the agent itself.
    TerminalAgent(Resume),
    /// Editor mode with a session: copy the resume command, then open.
    CopyAndOpen(Resume),
}

/// Port of `resolveOpenAction` (same rules, same tests).
pub fn resolve_open_action(resume_last_session: bool, already_open: bool, ide: IdeChoice, resume: Resume) -> OpenAction {
    if !resume_last_session || already_open {
        return OpenAction::Plain;
    }
    if ide == IdeChoice::Terminal {
        return OpenAction::TerminalAgent(resume);
    }
    if resume.session_id.is_none() && !resume.continue_session {
        return OpenAction::Plain;
    }
    OpenAction::CopyAndOpen(resume)
}

/// Quote a word for `sh` (single quotes) unless it is plainly safe.
pub fn sh_quote(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "/._-+:@".contains(c)) {
        return s.to_string();
    }
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// The agent's resume command (`claude --resume <id>`, `gemini --resume
/// latest`, plain `claude`…), optionally prefixed with a cd.
pub fn agent_command(resume: &Resume) -> String {
    let provider = resume.agent.provider();
    let mut cmd = provider.program().to_string();
    for arg in provider.resume_args(resume.session_id.as_deref(), resume.continue_session) {
        cmd.push(' ');
        cmd.push_str(&sh_quote(&arg));
    }
    match &resume.cwd {
        Some(dir) => format!("cd {} && {cmd}", sh_quote(dir)),
        None => cmd,
    }
}

pub fn copy_to_clipboard(text: &str) -> bool {
    let candidates: &[&[&str]] = if cfg!(target_os = "macos") {
        &[&["pbcopy"]]
    } else {
        &[&["wl-copy"], &["xclip", "-selection", "clipboard"], &["xsel", "--clipboard", "--input"]]
    };
    for argv in candidates {
        let Ok(mut child) = Command::new(argv[0]).args(&argv[1..]).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() else {
            continue;
        };
        let wrote = child.stdin.take().is_some_and(|mut i| i.write_all(text.as_bytes()).is_ok());
        if wrote && child.wait().is_ok_and(|s| s.success()) {
            info!("launch", "Copied '{text}' to clipboard");
            return true;
        }
    }
    warn!("launch", "Failed to copy to clipboard: no working clipboard tool");
    false
}

fn run(cmd: &mut Command) -> Result<(), String> {
    let out = cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).lines().next().unwrap_or("failed").to_string())
    }
}

/// The shell line for a custom editor command: `{path}` becomes the quoted
/// path, or the quoted path is appended when the command has no `{path}`.
pub fn custom_command(cmd: &str, path: &Path) -> String {
    let quoted = sh_quote(&path.to_string_lossy());
    let cmd = cmd.trim();
    if cmd.contains("{path}") { cmd.replace("{path}", &quoted) } else { format!("{cmd} {quoted}") }
}

/// Open `path` (a worktree, or a file) in the editor: a built-in one via its
/// CLI, falling back to `open -a` when the shell command isn't installed; a
/// custom one by running `custom_cmd` through `sh -c`.
pub fn open_in_editor(path: &Path, ide: IdeChoice, custom_cmd: &str) -> Result<(), String> {
    let (cli, app) = match ide.editor() {
        Some(e) => e,
        None if ide == IdeChoice::Terminal => return open_terminal(path, None).map(|_| ()),
        None => return open_custom(path, custom_cmd),
    };
    let result = run(Command::new(cli).arg(path)).or_else(|e| {
        if cfg!(target_os = "macos") {
            run(Command::new("open").args(["-a", app]).arg(path))
        } else {
            Err(e)
        }
    });
    match &result {
        Ok(()) => info!("launch", "Opened {} in {app}", path.display()),
        Err(e) => warn!("launch", "Failed to open {} in {app}: {e}", path.display()),
    }
    result.map_err(|e| format!("Couldn't open {app}: {e}"))
}

/// What viewing the logs opens, given the log files that exist (`paths::log_files`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogView {
    /// No log file yet.
    Missing,
    /// Only `debug.log`: open it in the editor.
    Edit(PathBuf),
    /// It has rotated too: show the folder, with this file selected.
    Reveal(PathBuf),
}

pub fn log_view(files: &[PathBuf]) -> LogView {
    match files {
        [] => LogView::Missing,
        [one] => LogView::Edit(one.clone()),
        [first, ..] => LogView::Reveal(first.clone()),
    }
}

/// Show `path` in the file manager: Finder with it selected on macOS, its
/// folder through `xdg-open` elsewhere.
pub fn reveal_in_file_manager(path: &Path) -> Result<(), String> {
    let result = if cfg!(target_os = "macos") {
        run(Command::new("open").arg("-R").arg(path))
    } else {
        let dir = if path.is_dir() { path } else { path.parent().unwrap_or(path) };
        run(Command::new("xdg-open").arg(dir))
    };
    match &result {
        Ok(()) => info!("launch", "Revealed {} in the file manager", path.display()),
        Err(e) => warn!("launch", "Failed to reveal {}: {e}", path.display()),
    }
    result.map_err(|e| format!("Couldn't show {}: {e}", path.display()))
}

/// How long a custom editor command gets to fail before it counts as started.
const CUSTOM_GRACE: std::time::Duration = std::time::Duration::from_millis(1500);

/// Run a command that may stay in the foreground (an editor binary that
/// doesn't detach): an early non-zero exit is an error, still running after
/// [`CUSTOM_GRACE`] is success and the process is left to run.
fn run_detached(cmd: &mut Command) -> Result<(), String> {
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map_err(|e| e.to_string())?;
    let start = std::time::Instant::now();
    while start.elapsed() < CUSTOM_GRACE {
        match child.try_wait() {
            Ok(Some(st)) if st.success() => return Ok(()),
            Ok(Some(st)) => return Err(match st.code() {
                Some(127) => "command not found".into(),
                Some(n) => format!("exited with status {n}"),
                None => "killed by a signal".into(),
            }),
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(50)),
            Err(e) => return Err(e.to_string()),
        }
    }
    // Reap it whenever it exits so it doesn't linger as a zombie.
    std::thread::spawn(move || child.wait());
    Ok(())
}

fn open_custom(path: &Path, cmd: &str) -> Result<(), String> {
    if cmd.trim().is_empty() {
        warn!("launch", "Open in is set to custom but ideCommand is empty");
        return Err("Open in is set to Custom command but no command is set (Settings → Editor command)".into());
    }
    let line = custom_command(cmd, path);
    let mut c = Command::new("sh");
    c.args(["-c", &line]);
    // A file (`am script edit`) opens from its directory.
    if let Some(dir) = if path.is_dir() { Some(path) } else { path.parent() } {
        c.current_dir(dir);
    }
    let result = run_detached(&mut c);
    match &result {
        Ok(()) => info!("launch", "Opened {} with custom command: {line}", path.display()),
        Err(e) => warn!("launch", "Custom command failed for {}: {line}: {e}", path.display()),
    }
    result.map_err(|e| format!("Couldn't run `{line}`: {e}"))
}

/// Which terminal app to drive. Inside cmux (its socket is in the
/// environment; it reports TERM_PROGRAM=ghostty) new terminals are cmux
/// workspaces. Inside an editor's integrated terminal (TERM_PROGRAM=vscode)
/// there is no app to drive, so use Terminal.app.
pub fn terminal_app() -> &'static str {
    let cmux = std::env::var("CMUX_SOCKET_PATH").is_ok_and(|s| !s.is_empty());
    terminal_app_for(std::env::var("TERM_PROGRAM").ok().as_deref(), cmux)
}

fn terminal_app_for(term_program: Option<&str>, cmux: bool) -> &'static str {
    match term_program {
        _ if cmux => "cmux",
        Some("iTerm.app") => "iTerm2",
        Some("ghostty") => "Ghostty",
        Some("WarpTerminal") => "Warp",
        _ => "Terminal",
    }
}

fn applescript_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The cmux CLI: the app bundle's copy that cmux names in our environment,
/// else `cmux` on `$PATH`.
fn cmux_cli() -> String {
    std::env::var("CMUX_BUNDLED_CLI_PATH").ok().filter(|p| Path::new(p).is_file()).unwrap_or_else(|| "cmux".into())
}

/// `cmux new-workspace` arguments. cmux runs `command` in the workspace's
/// login shell, which stays open after it (like the `exec $SHELL` the other
/// apps get), and leaves a new workspace in the background unless told to
/// focus it.
fn cmux_args(dir: &str, command: Option<&str>) -> Vec<String> {
    let mut args: Vec<String> = ["new-workspace", "--cwd", dir, "--focus", "true"].iter().map(|s| s.to_string()).collect();
    if let Some(cmd) = command {
        args.extend(["--command".into(), cmd.into()]);
    }
    args
}

/// Where [`open_terminal`] opened, for messages: `a new Ghostty window`.
fn opened_in(app: &str) -> String {
    match app {
        "cmux" => "a new cmux workspace".into(),
        _ => format!("a new {app} window"),
    }
}

/// Open a new terminal at `path`, optionally running `command`, and say
/// where (`a new cmux workspace`, `a new iTerm2 window`): a workspace inside
/// cmux, else a window or tab of the terminal app am runs in. On macOS, when
/// that app can't be driven (a Ghostty-based terminal without Ghostty.app,
/// automation refused) Terminal.app opens instead.
pub fn open_terminal(path: &Path, command: Option<&str>) -> Result<String, String> {
    let dir = path.to_string_lossy();
    let app = match terminal_app() {
        "cmux" => "cmux",
        _ if !cfg!(target_os = "macos") => "x-terminal-emulator",
        app => app,
    };
    let mut result = open_terminal_in(app, &dir, command).map(|()| app);
    if let Err(e) = &result
        && cfg!(target_os = "macos")
        && app != "Terminal"
    {
        warn!("launch", "Failed to open {app} at {dir}: {e}; trying Terminal");
        result = open_terminal_in("Terminal", &dir, command).map(|()| "Terminal").or(result);
    }
    match &result {
        Ok(opened) => info!("launch", "Opened {opened} at {dir} (command: {})", command.unwrap_or("-")),
        Err(e) => warn!("launch", "Failed to open {app} at {dir}: {e}"),
    }
    result.map(opened_in).map_err(|e| format!("Couldn't open {app}: {e}"))
}

fn open_terminal_in(app: &str, dir: &str, command: Option<&str>) -> Result<(), String> {
    let shell_cmd = match command {
        Some(c) => format!("cd {} && {c}", sh_quote(dir)),
        None => format!("cd {}", sh_quote(dir)),
    };
    match app {
        "cmux" => run(Command::new(cmux_cli()).args(cmux_args(dir, command))),
        "x-terminal-emulator" => {
            let mut c = Command::new("x-terminal-emulator");
            c.arg(format!("--working-directory={dir}"));
            if let Some(cmd) = command {
                c.args(["-e", "sh", "-c", cmd]);
            }
            run(&mut c)
        }
        "iTerm2" => {
            let cmd = applescript_string(&format!("/bin/zsh -lc {}", sh_quote(&format!("{shell_cmd}; exec $SHELL"))));
            let script = format!(
                "tell application \"iTerm2\"\n activate\n if (count of windows) > 0 then\n  tell current window to create tab with default profile command {cmd}\n else\n  create window with default profile command {cmd}\n end if\nend tell"
            );
            run(Command::new("osascript").arg("-e").arg(script))
        }
        "Ghostty" => {
            let mut c = Command::new("open");
            c.args(["-na", "Ghostty", "--args"]).arg(format!("--working-directory={dir}"));
            if let Some(cmd) = command {
                c.arg("-e").args(["/bin/zsh", "-lc", &format!("{cmd}; exec $SHELL")]);
            }
            run(&mut c)
        }
        "Warp" if command.is_none() => run(Command::new("open").args(["-a", "Warp", dir])),
        _ => {
            let script = format!(
                "tell application \"Terminal\"\n activate\n do script {}\nend tell",
                applescript_string(&shell_cmd)
            );
            run(Command::new("osascript").arg("-e").arg(script))
        }
    }
}

/// Bring the terminal app forward (a terminal is already open at the worktree).
pub fn focus_terminal_app() -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Ok(());
    }
    run(Command::new("osascript").arg("-e").arg(format!("tell application \"{}\" to activate", terminal_app())))
}

/// Resume info for a worktree. `known` is the agent the hooks last saw a
/// session from: its newest saved session, else "continue latest". With no
/// hook history, the newest session of any agent; with none at all, a fresh
/// `default_agent` session.
pub fn resume_for(root: &Path, known: Option<HarnessKind>, default_agent: HarnessKind) -> Resume {
    let newest = match known {
        Some(agent) => agent.provider().discover_sessions(root).into_iter().next(),
        None => discover_sessions(root).into_iter().next(),
    };
    let root_s = root.to_string_lossy();
    Resume {
        agent: newest.as_ref().map(|s| s.agent).or(known).unwrap_or(default_agent),
        cwd: newest.as_ref().map(|s| s.cwd.clone()).filter(|c| c.as_str() != root_s),
        session_id: newest.map(|s| s.id),
        continue_session: known.is_some(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resume(id: Option<&str>, cont: bool) -> Resume {
        Resume { session_id: id.map(str::to_owned), continue_session: cont, ..Default::default() }
    }

    #[test]
    fn cmux_is_picked_before_the_ghostty_it_reports() {
        assert_eq!(terminal_app_for(Some("ghostty"), true), "cmux");
        assert_eq!(terminal_app_for(None, true), "cmux");
        assert_eq!(terminal_app_for(Some("ghostty"), false), "Ghostty");
        assert_eq!(terminal_app_for(Some("iTerm.app"), false), "iTerm2");
        assert_eq!(terminal_app_for(Some("WarpTerminal"), false), "Warp");
        assert_eq!(terminal_app_for(Some("vscode"), false), "Terminal");
        assert_eq!(terminal_app_for(None, false), "Terminal");
    }

    #[test]
    fn cmux_opens_a_focused_workspace_running_the_command() {
        assert_eq!(cmux_args("/w/t", None), ["new-workspace", "--cwd", "/w/t", "--focus", "true"]);
        assert_eq!(
            cmux_args("/w/t", Some("AM_PR_NUMBER='61' /bin/sh '/s/r.review.sh'")),
            ["new-workspace", "--cwd", "/w/t", "--focus", "true", "--command", "AM_PR_NUMBER='61' /bin/sh '/s/r.review.sh'"]
        );
        assert_eq!(opened_in("cmux"), "a new cmux workspace");
        assert_eq!(opened_in("Ghostty"), "a new Ghostty window");
    }

    #[test]
    fn open_action_rules_match_ts() {
        use IdeChoice::*;
        assert_eq!(resolve_open_action(false, false, Vscode, resume(Some("s"), true)), OpenAction::Plain);
        assert_eq!(resolve_open_action(true, true, Vscode, resume(Some("s"), true)), OpenAction::Plain);
        assert_eq!(resolve_open_action(true, false, Vscode, resume(None, false)), OpenAction::Plain);
        assert_eq!(resolve_open_action(true, false, Terminal, resume(None, false)), OpenAction::TerminalAgent(resume(None, false)));
        assert_eq!(resolve_open_action(true, false, Cursor, resume(Some("s"), false)), OpenAction::CopyAndOpen(resume(Some("s"), false)));
        assert_eq!(resolve_open_action(true, false, Vscode, resume(None, true)), OpenAction::CopyAndOpen(resume(None, true)));
    }

    #[test]
    fn log_view_edits_one_file_and_reveals_rotated_logs() {
        let log = PathBuf::from("/d/debug.log");
        let old = PathBuf::from("/d/debug.log.1");
        assert_eq!(log_view(&[]), LogView::Missing);
        assert_eq!(log_view(std::slice::from_ref(&log)), LogView::Edit(log.clone()));
        // Only the rotated file left (debug.log not recreated yet): still one file.
        assert_eq!(log_view(std::slice::from_ref(&old)), LogView::Edit(old.clone()));
        assert_eq!(log_view(&[log.clone(), old]), LogView::Reveal(log));
    }

    #[test]
    fn agent_commands() {
        assert_eq!(agent_command(&resume(Some("abc-123"), true)), "claude --resume abc-123");
        assert_eq!(agent_command(&resume(None, true)), "claude -c");
        assert_eq!(agent_command(&resume(None, false)), "claude");
        let r = Resume { cwd: Some("/w/my pkg".into()), ..resume(Some("x"), false) };
        assert_eq!(agent_command(&r), "cd '/w/my pkg' && claude --resume x");
        let g = |id: Option<&str>, cont| Resume { agent: HarnessKind::GEMINI, ..resume(id, cont) };
        assert_eq!(agent_command(&g(Some("u-1"), true)), "gemini --resume u-1");
        assert_eq!(agent_command(&g(None, true)), "gemini --resume latest");
        assert_eq!(agent_command(&g(None, false)), "gemini");
    }

    #[test]
    fn quoting() {
        assert_eq!(sh_quote("/plain/path-1.2"), "/plain/path-1.2");
        assert_eq!(sh_quote("it's"), "'it'\\''s'");
        assert_eq!(applescript_string("a \"b\" \\c"), "\"a \\\"b\\\" \\\\c\"");
    }

    #[test]
    fn ide_setting_falls_back_to_vscode() {
        for (id, _) in &IdeChoice::ALL[1..] {
            assert_ne!(IdeChoice::from_setting(id), IdeChoice::Vscode, "{id}");
        }
        assert_eq!(IdeChoice::from_setting("bogus"), IdeChoice::Vscode);
        assert_eq!(IdeChoice::IDS.to_vec(), IdeChoice::ALL.map(|(id, _)| id).to_vec());
    }

    #[test]
    fn custom_command_places_the_quoted_path() {
        let p = Path::new("/w/it's here");
        assert_eq!(custom_command("subl -n", p), "subl -n '/w/it'\\''s here'");
        assert_eq!(custom_command(" open -a \"Sublime Text\" {path} ", p), "open -a \"Sublime Text\" '/w/it'\\''s here'");
        assert_eq!(custom_command("idea", Path::new("/w/x")), "idea /w/x");
    }

    #[test]
    fn custom_command_runs_and_reports_failures() {
        let dir = std::env::temp_dir();
        assert!(open_in_editor(&dir, IdeChoice::Custom, "  ").unwrap_err().contains("Editor command"));
        assert!(open_in_editor(&dir, IdeChoice::Custom, "true").is_ok());
        assert!(open_in_editor(&dir, IdeChoice::Custom, "exit 3 ;").unwrap_err().contains("status 3"));
        assert!(open_in_editor(&dir, IdeChoice::Custom, "am-no-such-editor-xyz").unwrap_err().contains("not found"));
    }
}
