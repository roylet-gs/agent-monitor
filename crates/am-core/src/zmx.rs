//! zmx (<https://zmx.sh>), terminal session persistence, driven through its
//! `zmx` CLI.
//!
//! What am relies on (zmx 1.x, checked against its source):
//! - One daemon per named session owns the PTY; `zmx attach <name>
//!   [command…]` creates the session (running `command`, else a login shell,
//!   in the caller's cwd) or reattaches to it, restoring the screen and
//!   scrollback. Several clients may attach at once.
//! - Inside a session `ZMX_SESSION` is the session's name, so am's hooks
//!   record it per agent session (`db::set_session_zmx`).
//! - `zmx list` prints one line per session, tab-separated `key=value` pairs:
//!   `name= pid= clients= created= [cwd=] [cmd=] [ended= exit_code=]` plus
//!   any labels; a line for a session that doesn't answer has `err=` instead
//!   of `pid=`, and the current session's line starts with `→ `.
//! - `zmx history <name> --scrollback <rows>` prints the session's screen
//!   plus that many scrollback rows as plain text.
//! - `zmx send <name> <text…>` writes raw bytes to the session's PTY (no
//!   newline added); `zmx kill <name>…` ends sessions.
//! - `ZMX_SESSION_PREFIX` is prepended to every name a command takes, and
//!   inside a session `attach` switches the current client rather than
//!   nesting, so both are removed from the environment of every call am
//!   makes (names am stores and passes are always the full names).
//!
//! Everything goes through a [`ZmxRunner`] so tests use fixtures; nothing
//! here may run on the UI thread.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::{debug, info};

const TIMEOUT: Duration = Duration::from_secs(5);

/// The prefix of the sessions am names itself (`am.<repo>.<branch>`).
pub const PREFIX: &str = "am.";

/// Longest name am gives a session. zmx's socket path (`<dir>/<name>`) must
/// fit in `sun_path` (104 bytes on macOS), and `$TMPDIR` there is ~50.
const MAX_NAME: usize = 40;

/// Result of one `zmx` call.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ZmxOutput {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

impl ZmxOutput {
    pub fn success(stdout: impl Into<String>) -> Self {
        ZmxOutput { ok: true, stdout: stdout.into(), stderr: String::new() }
    }
    pub fn failure(stderr: impl Into<String>) -> Self {
        ZmxOutput { ok: false, stdout: String::new(), stderr: stderr.into() }
    }

    fn into_result(self) -> Result<String, String> {
        if self.ok {
            Ok(self.stdout)
        } else {
            let msg = self.stderr.trim();
            Err(if msg.is_empty() { "zmx failed".to_string() } else { msg.trim_start_matches("error: ").to_string() })
        }
    }
}

/// Runs `zmx <args>`. Swapped for a fixture runner in tests.
pub trait ZmxRunner: Send + Sync {
    fn run(&self, args: &[String]) -> ZmxOutput;
}

/// The real `zmx` ([`cli_path`]).
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemZmx;

impl ZmxRunner for SystemZmx {
    fn run(&self, args: &[String]) -> ZmxOutput {
        let Some(cli) = cli_path() else {
            return ZmxOutput::failure("zmx isn't installed");
        };
        let spawned = Command::new(cli)
            .args(args)
            .env_remove("ZMX_SESSION")
            .env_remove("ZMX_SESSION_PREFIX")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();
        let mut child = match spawned {
            Ok(c) => c,
            Err(e) => return ZmxOutput::failure(format!("failed to run zmx: {e}")),
        };
        // Drain both pipes on threads so a long history can't deadlock on a full pipe.
        let mut out = child.stdout.take();
        let mut err = child.stderr.take();
        let t_out = std::thread::spawn(move || {
            let mut s = Vec::new();
            if let Some(o) = out.as_mut() {
                let _ = o.read_to_end(&mut s);
            }
            String::from_utf8_lossy(&s).into_owned()
        });
        let t_err = std::thread::spawn(move || {
            let mut s = Vec::new();
            if let Some(e) = err.as_mut() {
                let _ = e.read_to_end(&mut s);
            }
            String::from_utf8_lossy(&s).into_owned()
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
            Some(s) => ZmxOutput { ok: s.success(), stdout, stderr },
            None => ZmxOutput { ok: false, stdout, stderr: format!("zmx timed out after {}s", TIMEOUT.as_secs()) },
        }
    }
}

/// The `zmx` CLI: on `$PATH`, else where Homebrew, mise or `zig build
/// --prefix ~/.local` put it (a TUI started from a desktop launcher may have
/// a short `$PATH`).
pub fn cli_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("PATH")
        .and_then(|path| std::env::split_paths(&path).map(|d| d.join("zmx")).find(|p| p.is_file()))
    {
        return Some(p);
    }
    let home = crate::paths::home_dir();
    [PathBuf::from("/opt/homebrew/bin/zmx"), PathBuf::from("/usr/local/bin/zmx"), home.join(".local/bin/zmx")]
        .into_iter()
        .find(|p| p.is_file())
}

/// zmx is installed.
pub fn available() -> bool {
    cli_path().is_some()
}

/// The zmx session this process runs in (`ZMX_SESSION`), if any.
pub fn current_session() -> Option<String> {
    std::env::var("ZMX_SESSION").ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// One live session from `zmx list`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Session {
    pub name: String,
    pub pid: Option<u32>,
    pub clients: usize,
    pub cwd: Option<String>,
    /// The daemon didn't answer (`err=`): stale or busy.
    pub unreachable: bool,
}

/// Parse `zmx list` output (see the module docs).
pub fn parse_list(out: &str) -> Vec<Session> {
    out.lines()
        .filter_map(|line| {
            let line = line.trim_start_matches('→').trim();
            let mut s = Session::default();
            for field in line.split('\t') {
                let Some((k, v)) = field.split_once('=') else { continue };
                match k {
                    "name" => s.name = v.to_string(),
                    "pid" => s.pid = v.parse().ok(),
                    "clients" => s.clients = v.parse().unwrap_or(0),
                    "cwd" if !v.is_empty() => s.cwd = Some(v.to_string()),
                    "err" => s.unreachable = true,
                    _ => {}
                }
            }
            (!s.name.is_empty()).then_some(s)
        })
        .collect()
}

/// Every live session.
pub fn list(runner: &dyn ZmxRunner) -> Result<Vec<Session>, String> {
    let out = runner.run(&["list".to_string()]);
    // "no sessions found" goes to stderr with a zero exit.
    out.into_result().map(|s| parse_list(&s))
}

/// The last `rows` lines of a session's terminal (screen + scrollback), as
/// plain text with trailing blank lines dropped.
pub fn history(runner: &dyn ZmxRunner, name: &str, rows: usize) -> Result<Vec<String>, String> {
    let out = runner.run(&["history".into(), name.into(), "--scrollback".into(), rows.to_string()]).into_result()?;
    Ok(tail_lines(&out, rows))
}

/// The last `n` lines of `text`, trailing blank lines dropped, tabs expanded
/// and any stray control characters removed (the output is drawn in a pane).
pub fn tail_lines(text: &str, n: usize) -> Vec<String> {
    let mut lines: Vec<String> = text
        .lines()
        .map(|l| l.replace('\t', "    ").chars().filter(|c| !c.is_control()).collect::<String>().trim_end().to_string())
        .collect();
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    let start = lines.len().saturating_sub(n);
    lines.split_off(start)
}

/// Type `text` into a session (raw bytes; add `\r` for Enter).
pub fn send(runner: &dyn ZmxRunner, name: &str, text: &str) -> Result<(), String> {
    info!("zmx", "Sending {} byte(s) to zmx session {name}", text.len());
    runner.run(&["send".into(), name.into(), text.into()]).into_result().map(|_| ())
}

/// End sessions (exact names).
pub fn kill(runner: &dyn ZmxRunner, names: &[String]) -> Result<(), String> {
    if names.is_empty() {
        return Ok(());
    }
    info!("zmx", "Killing zmx session(s) {}", names.join(", "));
    let mut args = vec!["kill".to_string()];
    args.extend(names.iter().cloned());
    runner.run(&args).into_result().map(|_| ())
}

/// The sessions to end with a deleted worktree: of `recorded` (what its
/// agents' hooks saw) and the name am gives it, those am named (`am.`) that
/// are still live. A session the user named themselves may hold other work,
/// so it is never killed.
pub fn sessions_to_kill(live: &[Session], worktree_path: &Path, branch: &str, recorded: &[&str]) -> Vec<String> {
    let own = session_name(repo_dir(worktree_path), branch);
    let mut out: Vec<String> = Vec::new();
    for name in recorded.iter().copied().chain([own.as_str()]) {
        if name.starts_with(PREFIX) && live.iter().any(|s| s.name == name) && !out.iter().any(|o| o == name) {
            out.push(name.to_string());
        }
    }
    out
}

/// End a deleted worktree's am-named sessions ([`sessions_to_kill`]); quiet
/// when zmx isn't installed or nothing is running.
pub fn kill_worktree_sessions(worktree_path: &Path, branch: &str, recorded: &[&str]) {
    if !available() {
        return;
    }
    let runner = SystemZmx;
    let live = match list(&runner) {
        Ok(l) => l,
        Err(e) => {
            debug!("zmx", "Couldn't list zmx sessions to clean up {}: {e}", worktree_path.display());
            return;
        }
    };
    let names = sessions_to_kill(&live, worktree_path, branch, recorded);
    if let Err(e) = kill(&runner, &names) {
        crate::warn!("zmx", "Couldn't kill zmx session(s) {}: {e}", names.join(", "));
    }
}

/// The session am names for a worktree: `am.<repo>.<branch>`, limited to
/// characters safe in a file name and shell word, and shortened with a hash
/// when long (see [`MAX_NAME`]).
pub fn session_name(repo_path: &Path, branch: &str) -> String {
    let clean = |s: &str| -> String {
        let mut out: String =
            s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect();
        while out.contains("--") {
            out = out.replace("--", "-");
        }
        out.trim_matches('-').to_string()
    };
    let repo = repo_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let full = format!("{PREFIX}{}.{}", clean(&repo), clean(branch));
    if full.len() <= MAX_NAME {
        return full;
    }
    // FNV-1a: stable across runs and versions (unlike `DefaultHasher`).
    let hash = full.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3));
    let keep: String = full.chars().take(MAX_NAME - 7).collect();
    format!("{keep}-{:06x}", hash & 0xff_ffff)
}

/// The repo a worktree path belongs to: the directory above
/// `.claude/worktrees/<leaf>`, else the path itself (a main checkout).
pub fn repo_dir(worktree_path: &Path) -> &Path {
    let mut p = worktree_path;
    while let Some(parent) = p.parent() {
        if parent.ends_with(".claude/worktrees") {
            return parent.parent().and_then(Path::parent).unwrap_or(worktree_path);
        }
        p = parent;
    }
    worktree_path
}

/// The shell command that attaches to session `name`, creating it to run
/// `command` when it doesn't exist yet. The command runs through `sh -c` and
/// is followed by a login shell, so the session (and its scrollback) outlives
/// the agent. `ZMX_SESSION`/`ZMX_SESSION_PREFIX` are cleared so the new
/// terminal never switches an enclosing session or renames this one.
pub fn attach_command(name: &str, command: Option<&str>) -> String {
    let q = crate::launch::sh_quote;
    let zmx = cli_path().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|| "zmx".to_string());
    let mut cmd = format!("env -u ZMX_SESSION -u ZMX_SESSION_PREFIX {} attach {}", q(&zmx), q(name));
    if let Some(c) = command {
        let inner = format!("{c}; exec \"${{SHELL:-/bin/sh}}\" -l");
        cmd.push_str(&format!(" sh -c {}", q(&inner)));
    }
    debug!("zmx", "Attach command: {cmd}");
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Records each call and answers from a queue.
    struct Fixture {
        calls: Mutex<Vec<Vec<String>>>,
        answers: Mutex<Vec<ZmxOutput>>,
    }

    impl Fixture {
        fn new(answers: Vec<ZmxOutput>) -> Self {
            Fixture { calls: Mutex::new(vec![]), answers: Mutex::new(answers.into_iter().rev().collect()) }
        }
    }

    impl ZmxRunner for Fixture {
        fn run(&self, args: &[String]) -> ZmxOutput {
            self.calls.lock().unwrap().push(args.to_vec());
            self.answers.lock().unwrap().pop().unwrap_or_default()
        }
    }

    #[test]
    fn parses_list_lines_with_cwd_labels_errors_and_the_current_arrow() {
        let out = "→ name=am.web.feat\tpid=123\tclients=2\tcreated=17\tcwd=/r/web\tcmd=claude\tproject=x\n  name=dev\tpid=9\tclients=0\tcreated=1\n  name=old\terr=ConnectionRefused\tstatus=cleaning up\n\n";
        let list = parse_list(out);
        assert_eq!(
            list,
            vec![
                Session { name: "am.web.feat".into(), pid: Some(123), clients: 2, cwd: Some("/r/web".into()), unreachable: false },
                Session { name: "dev".into(), pid: Some(9), clients: 0, cwd: None, unreachable: false },
                Session { name: "old".into(), pid: None, clients: 0, cwd: None, unreachable: true },
            ]
        );
    }

    #[test]
    fn list_goes_through_the_runner_and_an_empty_listing_is_no_sessions() {
        let f = Fixture::new(vec![ZmxOutput::success("")]);
        assert_eq!(list(&f).unwrap(), vec![]);
        assert_eq!(f.calls.lock().unwrap()[0], vec!["list"]);
        let f = Fixture::new(vec![ZmxOutput::failure("error: boom\n")]);
        assert_eq!(list(&f).unwrap_err(), "boom");
    }

    #[test]
    fn history_asks_for_the_rows_and_keeps_the_last_non_blank_ones() {
        let f = Fixture::new(vec![ZmxOutput::success("a\nb\tc\nd\x07\n\n\n")]);
        assert_eq!(history(&f, "s", 2).unwrap(), vec!["b    c", "d"]);
        assert_eq!(f.calls.lock().unwrap()[0], vec!["history", "s", "--scrollback", "2"]);
    }

    #[test]
    fn send_and_kill_pass_names_through() {
        let f = Fixture::new(vec![ZmxOutput::success(""), ZmxOutput::success("")]);
        send(&f, "s", "y\r").unwrap();
        kill(&f, &["a".into(), "b".into()]).unwrap();
        kill(&f, &[]).unwrap();
        let calls = f.calls.lock().unwrap();
        assert_eq!(calls[0], vec!["send", "s", "y\r"]);
        assert_eq!(calls[1], vec!["kill", "a", "b"]);
        assert_eq!(calls.len(), 2, "nothing to kill makes no call");
    }

    #[test]
    fn session_names_are_safe_and_short() {
        assert_eq!(session_name(Path::new("/src/web"), "feature/login"), "am.web.feature-login");
        assert_eq!(session_name(Path::new("/src/my app"), "a//b"), "am.my-app.a-b");
        let long = session_name(Path::new("/src/agent-monitor"), "feature/a-really-long-branch-name-for-testing");
        assert_eq!(long.len(), MAX_NAME);
        assert!(long.starts_with("am.agent-monitor.feature-a-real"));
        assert_ne!(long, session_name(Path::new("/src/agent-monitor"), "feature/a-really-long-branch-name-for-testing2"));
    }

    #[test]
    fn repo_dir_of_a_linked_worktree_is_above_claude_worktrees() {
        assert_eq!(repo_dir(Path::new("/src/web/.claude/worktrees/feat-x")), Path::new("/src/web"));
        assert_eq!(repo_dir(Path::new("/src/web")), Path::new("/src/web"));
    }

    #[test]
    fn only_live_am_named_sessions_of_the_worktree_are_killed() {
        let live = parse_list("name=am.web.feat\tpid=1\tclients=0\tcreated=0\nname=dev\tpid=2\tclients=1\tcreated=0\nname=am.web.other\tpid=3\tclients=0\tcreated=0\n");
        let path = Path::new("/src/web/.claude/worktrees/feat");
        assert_eq!(sessions_to_kill(&live, path, "feat", &["dev", "am.web.feat"]), vec!["am.web.feat"]);
        assert_eq!(sessions_to_kill(&live, path, "gone", &[]), Vec::<String>::new());
    }

    #[test]
    fn attach_command_runs_the_agent_then_a_login_shell() {
        let cmd = attach_command("am.web.x", Some("claude --resume abc"));
        assert!(cmd.starts_with("env -u ZMX_SESSION -u ZMX_SESSION_PREFIX "), "{cmd}");
        assert!(cmd.contains(" attach am.web.x sh -c 'claude --resume abc; exec \"${SHELL:-/bin/sh}\" -l'"), "{cmd}");
        assert!(attach_command("am.web.x", None).ends_with(" attach am.web.x"));
    }
}
