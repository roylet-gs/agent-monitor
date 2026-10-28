//! CodeBuddy Code (Tencent, `codebuddy`) as a built-in plugin: the manifest
//! `codebuddy.json`, with its config dir picked at runtime.
//!
//! CodeBuddy's hooks copy Claude Code's: the nested `hooks` key of
//! `<config dir>/settings.json`, Claude's event names, payload keys
//! (`session_id`, `cwd`, `tool_name`, `notification_type`…) and tool names,
//! `$CODEBUDDY_PROJECT_DIR` for the project root, and `timeout` in seconds.
//! The config dir is `$CODEBUDDY_CONFIG_DIR` when set (and not blank),
//! else `~/.codebuddy`, as CodeBuddy itself decides; am installs the hooks
//! and detects the CLI there. Subagents are Claude's too: `SubagentStart`
//! carries `agent_id`/`agent_type`, a subagent's tool events and its
//! `SubagentStop` carry its `agent_id` with the parent's `session_id`, and
//! `Stop` fires only for the main session, so a turn that ends with
//! subagents running is `delegating`. Quirk: hook edits are reviewed in its
//! `/hooks` panel before they take effect. Transcripts live under
//! `<config dir>/projects/`, but their layout isn't documented, so resume
//! offers the sessions am's hooks recorded.

use std::ffi::OsString;
use std::path::PathBuf;

use super::embedded::Embedded;
use super::home_join;
use super::manifest::Manifest;

pub static CODEBUDDY: Embedded = Embedded::new(|| Manifest::builtin(include_str!("codebuddy.json")), None)
    .with_hook_file(|| (config_dir().join("settings.json"), "hooks"))
    .with_presence(|| config_dir().is_dir());

/// `$CODEBUDDY_CONFIG_DIR`, else `~/.codebuddy`.
fn config_dir() -> PathBuf {
    config_dir_from(std::env::var_os("CODEBUDDY_CONFIG_DIR"))
}

fn config_dir_from(var: Option<OsString>) -> PathBuf {
    // CodeBuddy: `env.CODEBUDDY_CONFIG_DIR?.trim() ? env.CODEBUDDY_CONFIG_DIR : join(homedir(), ".codebuddy")`.
    match var {
        Some(v) if !v.to_string_lossy().trim().is_empty() => PathBuf::from(v),
        _ => home_join(".codebuddy"),
    }
}

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
    fn the_config_dir_follows_codebuddy_config_dir() {
        assert_eq!(config_dir_from(Some("/cb/conf".into())), PathBuf::from("/cb/conf"));
        assert_eq!(config_dir_from(Some("  ".into())), home_join(".codebuddy"));
        assert_eq!(config_dir_from(None), home_join(".codebuddy"));
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
