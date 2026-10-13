//! GitHub Copilot CLI as a built-in plugin: the manifest `copilot.json`.
//!
//! Copilot reads every `*.json` in `~/.copilot/hooks/`, so am owns
//! `agent-monitor.json` there: `{"version": 1, "hooks": {event: [{"type",
//! "command", "timeoutSec"}]}}`. Registering the PascalCase event names makes
//! Copilot send Claude Code's payload (`session_id`, `cwd`, `tool_name` with
//! Claude's tool names). Its permission and notification events only exist in
//! the camelCase format, so a session shows `waiting` only for questions.
//! There's no project-dir variable: am-hook uses the payload's `cwd`. Resume
//! discovery uses the sessions am's hooks recorded.

use super::embedded::Embedded;
use super::manifest::Manifest;

pub static COPILOT: Embedded = Embedded::new(|| Manifest::builtin(include_str!("copilot.json")), None);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::Harness;
    use crate::status::{Status::*, map_event_to_status};
    use serde_json::{Value, json};

    #[test]
    fn a_copilot_turn_drives_the_status_rules() {
        let step = |cur, p: Value| map_event_to_status(&COPILOT.normalize(&p, None), cur, 0);
        let s = step(None, json!({"hook_event_name":"UserPromptSubmit","session_id":"c1","cwd":"/w","prompt":"hi"}));
        assert_eq!(s, Some(Executing));
        assert_eq!(step(s, json!({"hook_event_name":"PreToolUse","tool_name":"AskUserQuestion"})), Some(Waiting));
        assert_eq!(step(s, json!({"hook_event_name":"Stop","stop_reason":"end_turn"})), Some(Done));
    }

    #[test]
    fn hook_entries_are_flat_and_versioned() {
        let e = COPILOT.hook_entry("Stop", "\"/bin/am-hook\" hook-event", true);
        assert_eq!(e, json!({"type": "command", "command": "\"/bin/am-hook\" hook-event --agent copilot --worktree \"\" --event Stop", "timeoutSec": 5}));
        assert_eq!(COPILOT.hook_file_defaults().get("version"), Some(&json!(1)));
        assert!(COPILOT.settings_path().ends_with(".copilot/hooks/agent-monitor.json"));
        assert_eq!(COPILOT.resume_args(None, true), ["--continue"]);
    }
}
