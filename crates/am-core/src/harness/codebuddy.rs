//! CodeBuddy Code (Tencent, `codebuddy`) as a built-in plugin, entirely the
//! manifest `codebuddy.json`.
//!
//! CodeBuddy's hooks copy Claude Code's: the nested `hooks` key of
//! `~/.codebuddy/settings.json`, Claude's event names, payload keys
//! (`session_id`, `cwd`, `tool_name`, `notification_type`…) and tool names,
//! `$CODEBUDDY_PROJECT_DIR` for the project root, and `timeout` in seconds.
//! Quirks: its `SubagentStart`/`SubagentStop` payloads document no
//! `agent_id`, so subagents may not be tracked one by one (the events still
//! keep the session executing); hook edits are reviewed in its `/hooks` panel
//! before they take effect; CodeBuddy also honours a `$CODEBUDDY_CONFIG_DIR`
//! override, which am doesn't follow (it writes `~/.codebuddy`). Transcripts
//! live under `~/.codebuddy/projects/`, but their layout isn't documented, so
//! resume offers the sessions am's hooks recorded.

use super::embedded::Embedded;
use super::manifest::Manifest;

pub static CODEBUDDY: Embedded = Embedded::new(|| Manifest::builtin(include_str!("codebuddy.json")), None);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::Harness;
    use crate::status::{Status::*, map_event_to_status};
    use serde_json::{Value, json};

    #[test]
    fn a_codebuddy_turn_drives_the_status_rules() {
        let step = |cur, p: Value| map_event_to_status(&CODEBUDDY.normalize(&p, None), cur, 0);
        assert_eq!(step(None, json!({"hook_event_name":"SessionStart","session_id":"cb1","cwd":"/w","source":"startup"})), Some(Idle));
        let s = step(Some(Idle), json!({"hook_event_name":"UserPromptSubmit","session_id":"cb1","cwd":"/w","prompt":"hi"}));
        assert_eq!(s, Some(Executing));
        let e = CODEBUDDY.normalize(&json!({"hook_event_name":"PreToolUse","session_id":"cb1","tool_name":"Bash","tool_input":{"command":"ls"}}), None);
        assert_eq!((e.tool_name.as_deref(), e.session_id.as_deref()), (Some("Bash"), Some("cb1")));
        assert_eq!(step(s, json!({"hook_event_name":"PreToolUse","tool_name":"Bash"})), Some(Executing));
        assert_eq!(step(s, json!({"hook_event_name":"Notification","notification_type":"permission_prompt","message":"Allow Bash?"})), Some(Waiting));
        assert_eq!(step(s, json!({"hook_event_name":"PermissionRequest","tool_name":"Bash"})), Some(Waiting));
        assert_eq!(step(s, json!({"hook_event_name":"Stop","stop_hook_active":false})), Some(Done));
        assert_eq!(step(Some(Done), json!({"hook_event_name":"SessionEnd","reason":"exit"})), Some(Idle));
    }

    #[test]
    fn hook_entries_resume_and_process() {
        assert_eq!(CODEBUDDY.hooks_key(), "hooks");
        let e = CODEBUDDY.hook_entry("Stop", "\"/bin/am-hook\" hook-event", true);
        let h = &e["hooks"][0];
        assert_eq!((h["type"].as_str(), h["timeout"].as_u64()), (Some("command"), Some(5)));
        assert_eq!(h["command"], "\"/bin/am-hook\" hook-event --agent codebuddy --worktree \"$CODEBUDDY_PROJECT_DIR\" --event Stop");
        assert_eq!(CODEBUDDY.resume_args(Some("cb1"), false), ["--resume", "cb1"]);
        assert_eq!(CODEBUDDY.resume_args(None, true), ["--continue"]);
        assert!(CODEBUDDY.matches_process("codebuddy", &["codebuddy".into()]));
        assert!(CODEBUDDY.matches_process("node", &["node".into(), "/usr/lib/node_modules/@tencent-ai/codebuddy-code/bin/codebuddy".into()]));
        assert!(!CODEBUDDY.matches_process("claude", &["claude".into()]));
    }
}
