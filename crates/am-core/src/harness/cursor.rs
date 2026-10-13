//! Cursor (the editor's agent and the `cursor-agent` CLI) as a built-in
//! plugin: the manifest `cursor.json`.
//!
//! Hooks live in `~/.cursor/hooks.json` as `{"version": 1, "hooks": {event:
//! [{"command", "timeout"}]}}` with camelCase event names. Payloads carry
//! `conversation_id` (the session) and `workspace_roots` (no `cwd` on most
//! events), read through JSON pointers. There is no permission or
//! notification hook, so a Cursor session never shows `waiting`, and the CLI
//! may not fire every event the editor does.
//!
//! Cursor also runs Claude Code's hooks from `~/.claude/settings.json` by
//! default; am-hook drops those (their payload has `cursor_version`) so a
//! Cursor session isn't counted as a Claude one. Resume discovery uses the
//! sessions am's hooks recorded.

use super::embedded::Embedded;
use super::manifest::Manifest;

pub static CURSOR: Embedded = Embedded::new(|| Manifest::builtin(include_str!("cursor.json")), None);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::Harness;
    use crate::status::{Status::*, map_event_to_status};
    use serde_json::{Value, json};

    fn payload(event: &str) -> Value {
        json!({"hook_event_name": event, "conversation_id": "conv-1", "generation_id": "g1",
               "cursor_version": "2.4.0", "workspace_roots": ["/w/repo"], "model": "gpt-5"})
    }

    #[test]
    fn a_cursor_turn_drives_the_status_rules() {
        let step = |cur, p: Value| map_event_to_status(&CURSOR.normalize(&p, None), cur, 0);
        let e = CURSOR.normalize(&payload("beforeSubmitPrompt"), None);
        assert_eq!((e.event.as_str(), e.session_id.as_deref(), e.cwd.as_deref()), ("UserPromptSubmit", Some("conv-1"), Some("/w/repo")));
        let s = step(None, payload("beforeSubmitPrompt"));
        assert_eq!(s, Some(Executing));
        let mut tool = payload("preToolUse");
        tool["tool_name"] = "Shell".into();
        assert_eq!(CURSOR.normalize(&tool, None).tool_name.as_deref(), Some("Bash"));
        let mut stop = payload("stop");
        stop["status"] = "completed".into();
        assert_eq!(step(s, stop), Some(Done));
    }

    #[test]
    fn hook_entries_are_flat_and_versioned() {
        let e = CURSOR.hook_entry("stop", "\"/bin/am-hook\" hook-event", true);
        assert_eq!(e, json!({"command": "\"/bin/am-hook\" hook-event --agent cursor --worktree \"$CURSOR_PROJECT_DIR\" --event stop", "timeout": 5}));
        assert_eq!(CURSOR.hook_file_defaults().get("version"), Some(&json!(1)));
        assert_eq!(CURSOR.resume_args(Some("conv-1"), false), ["--resume", "conv-1"]);
        assert_eq!(CURSOR.program(), "cursor-agent");
    }
}
