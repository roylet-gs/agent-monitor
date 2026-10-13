//! Opening a worktree: IDE / terminal launch, agent session discovery and the
//! "resume last session" decision. Ported from `src/lib/ide-launcher.ts` and
//! `discoverWorktreeSessions` (`src/lib/claude-session.ts`).
//!
//! Deliberately no keystroke injection into terminals or editors: it needs
//! macOS Accessibility permission and fails silently (osascript error 1002)
//! without it. Editors get the resume command via the clipboard instead.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::SystemTime;

use crate::agents::{self, AgentKind};
use crate::{info, warn};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub id: String,
    /// Directory the session was started in (the worktree root or a subdir).
    pub cwd: String,
    pub modified: SystemTime,
    pub agent: AgentKind,
}

/// Every session of every enabled agent started at `root` or any
/// subdirectory, newest first.
pub fn discover_sessions(root: &Path) -> Vec<Session> {
    let enabled = agents::enabled(&crate::settings::load());
    let mut sessions: Vec<Session> = enabled.iter().flat_map(|k| k.provider().discover_sessions(root)).collect();
    sessions.sort_by_key(|s| std::cmp::Reverse(s.modified));
    sessions
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdeChoice {
    Cursor,
    Vscode,
    Terminal,
}

impl IdeChoice {
    pub fn from_setting(s: &str) -> IdeChoice {
        match s {
            "vscode" => IdeChoice::Vscode,
            "terminal" => IdeChoice::Terminal,
            _ => IdeChoice::Cursor,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Resume {
    /// Which agent CLI to start.
    pub agent: AgentKind,
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

/// Open the worktree in Cursor / VS Code via their CLI, falling back to
/// `open -a` when the shell command isn't installed.
pub fn open_in_editor(path: &Path, ide: IdeChoice) -> Result<(), String> {
    let (cli, app) = match ide {
        IdeChoice::Cursor => ("cursor", "Cursor"),
        IdeChoice::Vscode => ("code", "Visual Studio Code"),
        IdeChoice::Terminal => return open_terminal(path, None),
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

/// Which terminal app to drive. Inside an editor's integrated terminal
/// (TERM_PROGRAM=vscode) there is no app to drive, so use Terminal.app.
pub fn terminal_app() -> &'static str {
    match std::env::var("TERM_PROGRAM").as_deref() {
        Ok("iTerm.app") => "iTerm2",
        Ok("ghostty") => "Ghostty",
        Ok("WarpTerminal") => "Warp",
        _ => "Terminal",
    }
}

fn applescript_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Open a new terminal window/tab at `path`, optionally running `command`.
pub fn open_terminal(path: &Path, command: Option<&str>) -> Result<(), String> {
    let dir = path.to_string_lossy();
    let shell_cmd = match command {
        Some(c) => format!("cd {} && {c}", sh_quote(&dir)),
        None => format!("cd {}", sh_quote(&dir)),
    };
    let app = terminal_app();
    let result = if !cfg!(target_os = "macos") {
        let mut c = Command::new("x-terminal-emulator");
        c.arg(format!("--working-directory={dir}"));
        if let Some(cmd) = command {
            c.args(["-e", "sh", "-c", cmd]);
        }
        run(&mut c)
    } else {
        match app {
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
            "Warp" if command.is_none() => run(Command::new("open").args(["-a", "Warp"]).arg(path)),
            _ => {
                let script = format!(
                    "tell application \"Terminal\"\n activate\n do script {}\nend tell",
                    applescript_string(&shell_cmd)
                );
                run(Command::new("osascript").arg("-e").arg(script))
            }
        }
    };
    match &result {
        Ok(()) => info!("launch", "Opened {app} at {dir} (command: {})", command.unwrap_or("-")),
        Err(e) => warn!("launch", "Failed to open {app} at {dir}: {e}"),
    }
    result.map_err(|e| format!("Couldn't open {app}: {e}"))
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
pub fn resume_for(root: &Path, known: Option<AgentKind>, default_agent: AgentKind) -> Resume {
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
    fn agent_commands() {
        assert_eq!(agent_command(&resume(Some("abc-123"), true)), "claude --resume abc-123");
        assert_eq!(agent_command(&resume(None, true)), "claude -c");
        assert_eq!(agent_command(&resume(None, false)), "claude");
        let r = Resume { cwd: Some("/w/my pkg".into()), ..resume(Some("x"), false) };
        assert_eq!(agent_command(&r), "cd '/w/my pkg' && claude --resume x");
        let g = |id: Option<&str>, cont| Resume { agent: AgentKind::GEMINI, ..resume(id, cont) };
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
}
