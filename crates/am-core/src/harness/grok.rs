//! xAI's Grok CLI as a built-in plugin: the manifest `grok.json`, a payload
//! pre-pass and session discovery from Grok's session store.
//!
//! Grok reads every `*.json` in `~/.grok/hooks/` (`$GROK_HOME/hooks/` when
//! set), so am owns `agent-monitor.json` there, in Claude Code's nested
//! `{"hooks": {event: [{"hooks": [{"type", "command", "timeout"}]}]}}` shape
//! with the timeout in seconds. Quirks:
//! - Grok strips the environment from hook subprocesses and pre-checks plain
//!   `$VAR` references, so there's no project-dir variable: the command passes
//!   `--worktree ""` and am-hook uses the payload's `cwd`.
//! - Payload keys are camelCase (`sessionId`, `hookEventName`,
//!   `notificationType`, `toolName`…); [`snake_keys`] adds the snake_case
//!   spelling the manifest reads, so either spelling works.
//! - `SessionEnd` fires at the end of *every turn* right alongside `Stop`, not
//!   when the session closes. Both mapped to `Stop` would take a finished turn
//!   `done` → `idle`, and mapped to `SessionEnd` it would close the session,
//!   so am doesn't hook it: `Stop` ends the turn.
//! - `Notification` carries completion text ("Turn complete in 1.2s."), idle
//!   reminders ("waiting for input") and permission prompts
//!   (`notificationType: "permission_prompt"`). Grok sends a generic "Tool
//!   permission requested" prompt for every tool step, even in auto-approve
//!   mode where nothing waits on the user; that one is not counted as
//!   `waiting` (it would also pin the turn there, since `Stop` keeps a waiting
//!   session waiting). A specific one ("Grok needs permission to run rm") is.
//! - No `PostToolUse` or subagent events, so `executing` comes from
//!   `UserPromptSubmit` and `PreToolUse`.
//!
//! Sessions: `<grok home>/sessions/<percent-encoded cwd>/<sessionId>/
//! chat_history.jsonl`; resume is `grok -r <id>`.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::Value;

use super::embedded::Embedded;
use super::manifest::{Manifest, expand_home};
use super::{HarnessKind, home_join, is_within};
use crate::launch::Session;
use crate::status::HookEvent;

pub static GROK: Embedded = Embedded::new(|| Manifest::builtin(include_str!("grok.json")), Some(discover))
    .with_hook_file(|| (grok_home().join("hooks/agent-monitor.json"), "hooks"))
    .with_presence(|| grok_home().is_dir())
    .with_normalize(normalize);

/// `$GROK_HOME`, else `~/.grok`.
fn grok_home() -> PathBuf {
    match std::env::var("GROK_HOME") {
        Ok(h) if !h.trim().is_empty() => expand_home(h.trim()),
        _ => home_join(".grok"),
    }
}

/// The payload with a snake_case copy of every camelCase top-level key
/// (`sessionId` → `session_id`); a key already in snake_case wins.
fn snake_keys(payload: &Value) -> Value {
    let Some(obj) = payload.as_object() else { return payload.clone() };
    let mut out = obj.clone();
    for (k, v) in obj {
        if k.bytes().any(|b| b.is_ascii_uppercase()) {
            let mut snake = String::with_capacity(k.len() + 4);
            for c in k.chars() {
                if c.is_ascii_uppercase() {
                    snake.push('_');
                    snake.push(c.to_ascii_lowercase());
                } else {
                    snake.push(c);
                }
            }
            out.entry(snake).or_insert_with(|| v.clone());
        }
    }
    Value::Object(out)
}

/// The prompt Grok sends for every tool step, approval needed or not.
fn generic_permission(message: Option<&str>) -> bool {
    message.is_none_or(|m| m.trim().trim_end_matches('.').eq_ignore_ascii_case("tool permission requested"))
}

/// [`Embedded::with_normalize`]: the manifest on snake_case keys, then the
/// generic per-step permission prompt isn't counted as waiting.
fn normalize(m: &Manifest, payload: &Value, event_override: Option<&str>) -> HookEvent {
    let mut ev = m.normalize(&snake_keys(payload), event_override);
    if ev.is("Notification")
        && ev.notification_type.as_deref() == Some("permission_prompt")
        && generic_permission(ev.message.as_deref())
    {
        crate::debug!("agents", "grok: generic permission notification, not counted as waiting");
        ev.notification_type = None;
    }
    ev
}

fn discover(root: &Path) -> Vec<Session> {
    discover_sessions_in(&grok_home().join("sessions"), root)
}

/// Sessions started at `root` or below, newest first.
pub fn discover_sessions_in(sessions_dir: &Path, root: &Path) -> Vec<Session> {
    let root = root.to_string_lossy();
    let mut sessions = Vec::new();
    let Ok(projects) = std::fs::read_dir(sessions_dir) else { return sessions };
    for project in projects.flatten() {
        let Some(cwd) = percent_decode(&project.file_name().to_string_lossy()) else { continue };
        if !is_within(&cwd, &root) {
            continue;
        }
        let Ok(dirs) = std::fs::read_dir(project.path()) else { continue };
        for d in dirs.flatten() {
            let Ok(meta) = d.path().join("chat_history.jsonl").metadata() else { continue };
            let id = d.file_name().to_string_lossy().into_owned();
            if id.starts_with('.') {
                continue;
            }
            let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            sessions.push(Session { id, cwd: cwd.clone(), modified, agent: HarnessKind::GROK });
        }
    }
    sessions.sort_by_key(|s| std::cmp::Reverse(s.modified));
    sessions
}

/// `%2Fw%2Frepo` → `/w/repo` (Grok percent-encodes every byte outside
/// `A-Za-z0-9-._~`). `None` for a malformed name.
fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::Harness;
    use crate::status::{Status::*, map_event_to_status};
    use serde_json::json;

    fn step(cur: Option<crate::status::Status>, p: Value) -> Option<crate::status::Status> {
        map_event_to_status(&GROK.normalize(&p, None), cur, 0)
    }

    #[test]
    fn a_grok_turn_drives_the_status_rules() {
        let base = |e: &str| json!({"hookEventName": e, "sessionId": "g-1", "cwd": "/w/repo"});
        let s = step(None, base("SessionStart"));
        assert_eq!(s, Some(Idle));
        let s = step(s, base("UserPromptSubmit"));
        assert_eq!(s, Some(Executing));
        let mut tool = base("PreToolUse");
        tool["toolName"] = "bash".into();
        tool["toolInput"] = json!({"command": "ls"});
        let e = GROK.normalize(&tool, None);
        assert_eq!((e.session_id.as_deref(), e.cwd.as_deref(), e.tool_name.as_deref()), (Some("g-1"), Some("/w/repo"), Some("bash")));
        assert_eq!(e.tool_input, Some(json!({"command": "ls"})));
        assert_eq!(step(s, tool), Some(Executing));

        // The per-step generic prompt isn't waiting; a specific one is.
        let note = |msg: &str| {
            let mut n = base("Notification");
            n["notificationType"] = "permission_prompt".into();
            n["message"] = msg.into();
            n
        };
        assert_eq!(step(s, note("Tool permission requested")), None);
        assert_eq!(step(s, note("Grok needs permission to run rm")), Some(Waiting));
        // Completion text and idle reminders carry no type: no change.
        let mut done = base("Notification");
        done["message"] = "Turn complete in 1.2s.".into();
        assert_eq!(step(s, done), None);

        let s = step(s, base("Stop"));
        assert_eq!(s, Some(Done));
        // snake_case payloads read the same.
        let e = GROK.normalize(&json!({"session_id": "g-2", "cwd": "/w"}), Some("Stop"));
        assert_eq!((e.event.as_str(), e.session_id.as_deref()), ("Stop", Some("g-2")));
    }

    #[test]
    fn hook_entries_are_nested_with_seconds_and_no_project_var() {
        let e = GROK.hook_entry("Stop", "\"/bin/am-hook\" hook-event", true);
        assert_eq!(
            e,
            json!({"hooks": [{"type": "command", "command": "\"/bin/am-hook\" hook-event --agent grok --worktree \"\" --event Stop", "timeout": 5}]})
        );
        assert_eq!(GROK.hook_events(), ["SessionStart", "UserPromptSubmit", "PreToolUse", "Notification", "Stop"]);
        assert!(!GROK.hook_events().iter().any(|e| e == "SessionEnd"), "SessionEnd fires every turn");
        assert!(GROK.settings_path().ends_with("hooks/agent-monitor.json"));
        assert_eq!(GROK.hooks_key(), "hooks");
        assert_eq!(GROK.resume_args(Some("g-1"), false), ["-r", "g-1"]);
        assert!(GROK.resume_args(None, true).is_empty());
        assert!(GROK.matches_process("grok", &["grok".into()]));
        assert!(!GROK.matches_process("node", &["node".into(), "server.js".into()]));
    }

    #[test]
    fn sessions_are_found_under_the_encoded_cwd() {
        let dir = tempfile::tempdir().unwrap();
        let mk = |cwd: &str, id: &str| {
            let encoded: String = cwd
                .bytes()
                .map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
                .collect();
            let d = dir.path().join(encoded).join(id);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("chat_history.jsonl"), "{}\n").unwrap();
        };
        mk("/w/repo", "a");
        mk("/w/repo/pkg", "b");
        mk("/w/repo-other", "c");
        std::fs::create_dir_all(dir.path().join("%2Fw%2Frepo").join("no-history")).unwrap();
        let mut ids: Vec<String> = discover_sessions_in(dir.path(), Path::new("/w/repo")).into_iter().map(|s| s.id).collect();
        ids.sort();
        assert_eq!(ids, ["a", "b"]);
        assert_eq!(percent_decode("%2Fw%2Fmy%20repo").as_deref(), Some("/w/my repo"));
        assert_eq!(percent_decode("%2"), None);
    }
}
