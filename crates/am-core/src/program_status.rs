//! The Program Status Protocol (OSC 7501,
//! <https://www.superlogical.com/rex/docs/build/program-status>): a program
//! tells the terminal it runs in whether it is working, blocked on the user or
//! done, and the terminal shows it (an icon, an inbox, a notification).
//!
//! am only writes it, from the hook, on behalf of agent CLIs that don't report
//! it themselves (`Harness::reports_program_status`; Claude Code does since
//! 2.1.295). Nothing can read the records back, so the dashboard keeps using
//! the hooks. Opt-in (`programStatus`): the report goes to `/dev/tty`, the
//! terminal the agent draws on, so a badly timed write could land between two
//! of its own writes.
//!
//! One report per status change, the root record only (one agent per
//! terminal). Terminals that don't know OSC 7501 ignore it, and the hook
//! can't run the `?` feature query (the reply would land in the agent's
//! input), so there is no detection.

use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;

use crate::status::{HookEvent, Status};

/// Longest `msg` sent, in characters (the spec allows 2048 bytes decoded).
const MSG_CHARS: usize = 200;

/// Why a `blocked` program waits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blocked {
    Permission,
    Question,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Idle,
    Working,
    Blocked(Blocked),
    Done,
    /// Remove the record (the session ended).
    Clear,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub state: State,
    /// Stable program id (`app=`); left out when it isn't a valid one.
    pub app: Option<String>,
    /// One line for the terminal to show (`msg=`, base64 on the wire).
    pub msg: Option<String>,
}

/// The report for a session that just moved to `status` (or, with
/// `is_open` false, ended) on `ev`.
pub fn report(app: &str, status: Status, is_open: bool, ev: &HookEvent) -> Report {
    let state = if !is_open {
        State::Clear
    } else {
        match status {
            Status::Idle => State::Idle,
            Status::Executing | Status::Planning | Status::Delegating => State::Working,
            Status::Done => State::Done,
            Status::Waiting => State::Blocked(if ev.needs_user() || ev.permission_prompt { Blocked::Permission } else { Blocked::Question }),
        }
    };
    let msg = match state {
        State::Blocked(kind) => Some(blocked_msg(kind, ev)),
        _ => None,
    };
    let app = valid_app(app).then(|| app.to_owned());
    Report { state, app, msg }
}

fn blocked_msg(kind: Blocked, ev: &HookEvent) -> String {
    if let Some(m) = ev.message.as_deref().map(str::trim).filter(|m| !m.is_empty()) {
        return one_line(m);
    }
    if kind == Blocked::Permission
        && let Some(tool) = ev.tool_name.as_deref()
    {
        return match crate::status::summarize_tool_input(tool, ev.tool_input.as_ref(), ev.cwd.as_deref()) {
            Some(what) => one_line(&format!("Allow {tool}: {what}?")),
            None => one_line(&format!("Allow {tool}?")),
        };
    }
    match kind {
        Blocked::Permission => "Needs your permission".into(),
        Blocked::Question => "Waiting for your answer".into(),
    }
}

/// No control characters (the terminal must refuse them), one line, short.
fn one_line(s: &str) -> String {
    let flat: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    crate::status::truncate(flat.split_whitespace().collect::<Vec<_>>().join(" ").as_str(), MSG_CHARS)
}

/// `app` values the spec accepts: 1–32 bytes of letters, digits and `_ . + -`.
fn valid_app(app: &str) -> bool {
    (1..=32).contains(&app.len()) && app.bytes().all(|b| b.is_ascii_alphanumeric() || b"_.+-".contains(&b))
}

impl Report {
    /// The `key=value:…` body.
    pub fn body(&self) -> String {
        let state = match &self.state {
            State::Idle => "idle",
            State::Working => "working",
            State::Blocked(_) => "blocked",
            State::Done => "done",
            State::Clear => "clear",
        };
        let mut body = format!("state={state}");
        if let State::Blocked(kind) = self.state {
            body.push_str(match kind {
                Blocked::Permission => ":kind=permission",
                Blocked::Question => ":kind=question",
            });
        }
        if let Some(app) = &self.app
            && self.state != State::Clear
        {
            body.push_str(":app=");
            body.push_str(app);
        }
        if let Some(msg) = &self.msg {
            body.push_str(":msg=");
            body.push_str(&base64(msg.as_bytes()));
        }
        body
    }

    /// The full escape sequence, wrapped for tmux's passthrough when inside
    /// tmux (`allow-passthrough on` lets it reach the outer terminal; without
    /// it tmux drops it).
    pub fn sequence(&self, tmux: bool) -> Vec<u8> {
        let osc = format!("\x1b]7501;{}\x1b\\", self.body());
        if !tmux {
            return osc.into_bytes();
        }
        format!("\x1bPtmux;{}\x1b\\", osc.replace('\x1b', "\x1b\x1b")).into_bytes()
    }

    /// Write the report to the controlling terminal in one `write`.
    /// Non-blocking, so a terminal that stopped reading (`ctrl-s`) can't hold
    /// up the hook. Fails without a controlling terminal (a hook an IDE runs).
    /// `AM_PROGRAM_STATUS_TTY` names another file (tests only).
    pub fn emit(&self) -> std::io::Result<()> {
        let tmux = std::env::var_os("TMUX").is_some_and(|v| !v.is_empty());
        let bytes = self.sequence(tmux);
        let path = std::env::var_os("AM_PROGRAM_STATUS_TTY").filter(|p| !p.is_empty()).unwrap_or_else(|| "/dev/tty".into());
        let mut tty = std::fs::OpenOptions::new()
            .append(true)
            .custom_flags(libc::O_NOCTTY | libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open(path)?;
        let n = tty.write(&bytes)?;
        if n < bytes.len() {
            return Err(std::io::Error::new(std::io::ErrorKind::WriteZero, format!("short write ({n} of {} bytes)", bytes.len())));
        }
        Ok(())
    }
}

/// Standard base64 with padding.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ev(name: &str) -> HookEvent {
        HookEvent::from_json(&json!({"session_id": "s"}), Some(name))
    }

    #[test]
    fn base64_matches_the_standard_alphabet_and_padding() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64("Apply 3 to add, 1 to change, 0 to destroy?".as_bytes()), "QXBwbHkgMyB0byBhZGQsIDEgdG8gY2hhbmdlLCAwIHRvIGRlc3Ryb3k/");
    }

    #[test]
    fn statuses_map_onto_the_protocol_states() {
        let body = |s, open| report("gemini", s, open, &ev("PreToolUse")).body();
        assert_eq!(body(Status::Executing, true), "state=working:app=gemini");
        assert_eq!(body(Status::Planning, true), "state=working:app=gemini");
        assert_eq!(body(Status::Delegating, true), "state=working:app=gemini");
        assert_eq!(body(Status::Done, true), "state=done:app=gemini");
        assert_eq!(body(Status::Idle, true), "state=idle:app=gemini");
        assert_eq!(body(Status::Idle, false), "state=clear");
    }

    #[test]
    fn a_permission_prompt_is_blocked_with_its_message() {
        let mut n = ev("Notification");
        n.notification_type = Some("permission_prompt".into());
        n.message = Some("Gemini needs\npermission to run `rm`".into());
        let r = report("gemini", Status::Waiting, true, &n);
        assert_eq!(r.state, State::Blocked(Blocked::Permission));
        assert_eq!(r.msg.as_deref(), Some("Gemini needs permission to run `rm`"));
        assert_eq!(r.body(), format!("state=blocked:kind=permission:app=gemini:msg={}", base64(b"Gemini needs permission to run `rm`")));
    }

    #[test]
    fn a_permission_request_names_the_tool_and_anything_else_is_a_question() {
        let mut p = ev("PermissionRequest");
        p.tool_name = Some("Bash".into());
        p.tool_input = Some(json!({"command": "cargo test"}));
        assert_eq!(report("codex", Status::Waiting, true, &p).msg.as_deref(), Some("Allow Bash: cargo test?"));

        let stop = ev("Stop");
        let r = report("codex", Status::Waiting, true, &stop);
        assert_eq!(r.state, State::Blocked(Blocked::Question));
        assert_eq!(r.msg.as_deref(), Some("Waiting for your answer"));
    }

    #[test]
    fn an_invalid_app_id_is_left_out() {
        assert_eq!(report("my cli", Status::Done, true, &ev("Stop")).body(), "state=done");
        assert_eq!(report(&"x".repeat(33), Status::Done, true, &ev("Stop")).body(), "state=done");
    }

    #[test]
    fn the_sequence_is_wrapped_for_tmux_passthrough() {
        let r = report("qwen", Status::Done, true, &ev("Stop"));
        assert_eq!(r.sequence(false), b"\x1b]7501;state=done:app=qwen\x1b\\");
        assert_eq!(r.sequence(true), b"\x1bPtmux;\x1b\x1b]7501;state=done:app=qwen\x1b\x1b\\\x1b\\");
    }
}
