//! Muse Code as a built-in plugin: the manifest `muse.json`.
//!
//! Muse's hooks use Claude Code's nested entry shape and event names with a
//! Claude-style JSON payload on stdin, timeouts in seconds. Quirks:
//! - The hooks live in `$XDG_CONFIG_HOME/muse/settings.json`, else
//!   `~/.config/muse/settings.json`, so the file is picked at runtime.
//! - Muse refuses to start with a settings file that has no
//!   `"schema_version"`, so the manifest's `file` adds `schema_version: 1`
//!   when it's missing (an existing value is kept).
//! - There is no project-dir variable: the command passes `--worktree ""` and
//!   am-hook uses the payload's `cwd`.
//! - Only session/turn events are installed (`SessionStart`,
//!   `UserPromptSubmit`, `Notification`, `Stop`, `StopFailure`, `SessionEnd`),
//!   so a turn reads `executing` with no tool detail, and `waiting` depends
//!   on Muse sending `notification_type: permission_prompt` (unverified).
//! - Internal child sessions (reminders, delegated subagents) fire the same
//!   hooks with `turn_id == session_id`; top-level turns carry a distinct turn
//!   id and lifecycle events none. Their events are ignored (as cmux does):
//!   counted, they'd show up as extra sessions in the worktree and, being the
//!   newest, could push the real session out of the per-process session cap.
//! - Muse runs hooks through the user's login `$SHELL -c` with a scrubbed
//!   environment. The command is a quoted absolute path plus plain arguments,
//!   which sh/bash/zsh/fish all run as-is; nushell needs `^` in front of a
//!   quoted command path, so nushell users must wrap it themselves. am-hook
//!   finds its data dir through `$HOME`, falling back to the passwd entry.
//!
//! Resume is `muse resume <id>`. Muse indexes sessions in its own
//! `session-index.db`, whose schema isn't documented, so resume discovery uses
//! the sessions am's hooks recorded.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::embedded::Embedded;
use super::home_join;
use super::manifest::Manifest;
use crate::status::HookEvent;

pub static MUSE: Embedded = Embedded::new(|| Manifest::builtin(include_str!("muse.json")), None)
    .with_hook_file(hook_file)
    // The hook file is picked at runtime (`$XDG_CONFIG_HOME`), so its
    // directory, not a fixed `configDir`, says Muse is installed.
    .with_presence(|| hook_file().0.parent().is_some_and(Path::is_dir))
    .with_normalize(normalize);

fn hook_file() -> (PathBuf, &'static str) {
    (pick_hook_file(std::env::var_os("XDG_CONFIG_HOME").as_deref().map(Path::new)), "hooks")
}

/// `$XDG_CONFIG_HOME/muse/settings.json` when that's set to an absolute path
/// (the XDG rule), else `~/.config/muse/settings.json`.
fn pick_hook_file(xdg: Option<&Path>) -> PathBuf {
    match xdg.filter(|p| p.is_absolute()) {
        Some(dir) => dir.join("muse/settings.json"),
        None => home_join(".config/muse/settings.json"),
    }
}

/// Child-session events become an `ignored` event with no session and no
/// `cwd`, which am-hook skips; everything else goes through the manifest.
fn normalize(m: &Manifest, payload: &Value, event_override: Option<&str>) -> HookEvent {
    if is_child_session(payload) {
        crate::debug!("agents", "muse: ignoring an internal child-session event ({event_override:?})");
        return HookEvent { event: "ignored".into(), ..HookEvent::default() };
    }
    m.normalize(payload, event_override)
}

/// A Muse internal child session reports its own session id as the turn id.
fn is_child_session(payload: &Value) -> bool {
    let s = |k: &str| payload.get(k).and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty());
    matches!((s("session_id"), s("turn_id")), (Some(sid), Some(tid)) if sid == tid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{Harness, HarnessKind};
    use crate::status::{Status::*, map_event_to_status};
    use serde_json::json;

    #[test]
    fn a_muse_turn_drives_the_status_rules() {
        let step = |cur, p: Value| map_event_to_status(&MUSE.normalize(&p, None), cur, 0);
        let start = MUSE.normalize(&json!({"hook_event_name":"SessionStart","session_id":"m1","cwd":"/w"}), None);
        assert_eq!((start.session_id.as_deref(), start.cwd.as_deref()), (Some("m1"), Some("/w")));
        let s = step(None, json!({"hook_event_name":"UserPromptSubmit","session_id":"m1","turn_id":"t1","cwd":"/w"}));
        assert_eq!(s, Some(Executing));
        let ask = json!({"hook_event_name":"Notification","session_id":"m1","notification_type":"permission_prompt"});
        assert_eq!(step(s, ask), Some(Waiting));
        assert_eq!(step(s, json!({"hook_event_name":"Stop","session_id":"m1","turn_id":"t1"})), Some(Done));
        assert_eq!(step(s, json!({"hook_event_name":"StopFailure","session_id":"m1","turn_id":"t1"})), Some(Idle));
        // The `--event` flag names the event when the payload doesn't.
        assert_eq!(MUSE.normalize(&json!({"session_id":"m1"}), Some("SessionEnd")).event, "SessionEnd");
    }

    #[test]
    fn child_sessions_are_ignored() {
        let child = json!({"hook_event_name":"UserPromptSubmit","session_id":"c1","turn_id":"c1","cwd":"/w"});
        let e = MUSE.normalize(&child, Some("UserPromptSubmit"));
        assert_eq!((e.session_id.as_deref(), e.cwd.as_deref()), (None, None));
        assert_eq!(map_event_to_status(&e, Some(Done), 0), None);
        assert!(is_child_session(&json!({"session_id":" c1 ","turn_id":"c1"})));
        assert!(!is_child_session(&json!({"session_id":"m1","turn_id":"t1"})));
        assert!(!is_child_session(&json!({"session_id":"m1"})));
        assert!(!is_child_session(&json!({"session_id":"","turn_id":""})));
    }

    #[test]
    fn hook_file_follows_xdg() {
        assert_eq!(pick_hook_file(Some(Path::new("/x/cfg"))), PathBuf::from("/x/cfg/muse/settings.json"));
        assert_eq!(pick_hook_file(Some(Path::new("rel"))), home_join(".config/muse/settings.json"));
        assert_eq!(pick_hook_file(None), home_join(".config/muse/settings.json"));
        assert_eq!(MUSE.hooks_key(), "hooks");
    }

    #[test]
    fn hook_entries_resume_and_processes() {
        let e = MUSE.hook_entry("Stop", "\"/bin/am-hook\" hook-event", true);
        let h = &e["hooks"][0];
        assert_eq!(h["timeout"].as_u64(), Some(10));
        assert_eq!(h["command"], "\"/bin/am-hook\" hook-event --agent muse --worktree \"\" --event Stop");
        assert_eq!(MUSE.hook_file_defaults().get("schema_version"), Some(&json!(1)));
        assert_eq!(
            MUSE.hook_events(),
            ["SessionStart", "SessionEnd", "UserPromptSubmit", "Notification", "Stop", "StopFailure"]
        );
        assert_eq!(MUSE.resume_args(Some("m1"), false), ["resume", "m1"]);
        assert!(MUSE.resume_args(None, true).is_empty());
        assert!(MUSE.matches_process("muse", &[]));
        assert!(MUSE.matches_process("node", &["node".into(), "/opt/bin/muse".into()]));
        assert!(!MUSE.matches_process("vite", &["vite".into()]));
        assert_eq!(HarnessKind::get("muse"), Some(HarnessKind::MUSE));
    }
}
