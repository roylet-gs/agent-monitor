//! Qwen Code as a built-in plugin, entirely the manifest `qwen.json`.
//!
//! Qwen Code is a Gemini CLI fork whose hooks follow Claude Code's layout,
//! event names and payload keys (`~/.qwen/settings.json`, `$QWEN_PROJECT_DIR`),
//! including subagent and permission events. Tool names are its runtime ids
//! (`run_shell_command`, `write_file`…), mapped onto Claude's. Resume
//! discovery uses the sessions am's hooks recorded.

use super::embedded::Embedded;
use super::manifest::Manifest;

pub static QWEN: Embedded = Embedded::new(|| Manifest::builtin(include_str!("qwen.json")), None);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::Harness;
    use crate::status::{Status::*, map_event_to_status};
    use serde_json::{Value, json};

    #[test]
    fn a_qwen_turn_drives_the_status_rules() {
        let step = |cur, p: Value| map_event_to_status(&QWEN.normalize(&p, None), cur, 0);
        let s = step(None, json!({"hook_event_name":"UserPromptSubmit","session_id":"q1","cwd":"/w"}));
        assert_eq!(s, Some(Executing));
        let e = QWEN.normalize(&json!({"hook_event_name":"PreToolUse","tool_name":"run_shell_command","tool_input":{"command":"ls"}}), None);
        assert_eq!(e.tool_name.as_deref(), Some("Bash"));
        assert_eq!(step(s, json!({"hook_event_name":"Notification","notification_type":"permission_prompt"})), Some(Waiting));
        assert_eq!(step(s, json!({"hook_event_name":"Stop","last_assistant_message":"done"})), Some(Done));
    }

    #[test]
    fn hook_entries_and_resume() {
        let e = QWEN.hook_entry("Stop", "\"/bin/am-hook\" hook-event", true);
        let h = &e["hooks"][0];
        assert_eq!((h["timeout"].as_u64(), h["name"].as_str()), (Some(5), Some("agent-monitor")));
        assert_eq!(h["command"], "\"/bin/am-hook\" hook-event --agent qwen --worktree \"$QWEN_PROJECT_DIR\" --event Stop");
        assert_eq!(QWEN.resume_args(Some("q1"), false), ["--resume", "q1"]);
        assert_eq!(QWEN.resume_args(None, true), ["--continue"]);
        assert!(QWEN.matches_process("node", &["node".into(), "/usr/lib/node_modules/@qwen-code/qwen-code/cli.js".into()]));
    }
}
