//! Google Antigravity CLI (`agy`) as a built-in plugin: the manifest
//! `antigravity.json` plus one payload rule.
//!
//! Hooks live in `~/.gemini/config/hooks.json` (a different file from Gemini
//! CLI's `~/.gemini/settings.json`), grouped under a named top-level key: am
//! owns `"agent-monitor"` and leaves other groups (cmux's `"cmux"`…) alone.
//! Lifecycle events take flat `{"type": "command", "command", "timeout"}`
//! handlers with the timeout in seconds; the tool events would need a
//! `{"matcher", "hooks"}` wrapper and are not installed (they would cost a
//! process per tool call and carry nothing the status rules need).
//!
//! Payloads are camelCase: `conversationId` is the session and
//! `workspacePaths[0]` the project (there is no project-dir variable, so the
//! command passes `--worktree ""`). `PreInvocation` fires before every model
//! invocation (including the ones after tool results, `invocationNum` > 0),
//! which all read as `UserPromptSubmit` → executing. `Stop` carries
//! `fullyIdle`; `false` means background work is still running, so that stop
//! is read as activity (`PostToolUse`) and the session stays executing until
//! the idle `Stop`. `SessionStart` (agy ≥ 1.1.1) fires once per new
//! conversation, not on `agy --conversation` resumes. There is no permission
//! or notification hook, so a session never shows `waiting`, and no session-end
//! hook (`ends_sessions` is false), so am-hook closes the owner's settled
//! (`done`/`idle`) Antigravity sessions when a new one starts. The process
//! is `agy`, or `antigravity` as the release tarball names it.
//!
//! cmux also registers undocumented `Notification`, `turn-completion` and a
//! per-turn `SessionEnd`; am doesn't, since each would be a second `Stop` for
//! the same turn (done → idle). Presence is agy's own state dir
//! `~/.gemini/antigravity-cli`, not `~/.gemini` (Gemini CLI's). Conversations
//! live in `~/.gemini/antigravity-cli/brain/<id>/` with no cwd in the path, so
//! resume discovery uses the sessions am's hooks recorded.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use super::embedded::Embedded;
use super::manifest::Manifest;
use super::{Harness, McpConfig};
use crate::launch::Session;
use crate::status::HookEvent;

static INNER: Embedded = Embedded::new(|| Manifest::builtin(include_str!("antigravity.json")), None);

pub struct Antigravity;

pub static ANTIGRAVITY: Antigravity = Antigravity;

impl Harness for Antigravity {
    fn id(&self) -> &str {
        INNER.id()
    }

    fn name(&self) -> &str {
        INNER.name()
    }

    fn short_name(&self) -> &str {
        INNER.short_name()
    }

    fn program(&self) -> &str {
        INNER.program()
    }

    fn notes(&self) -> Option<&str> {
        INNER.notes()
    }

    fn is_present(&self) -> bool {
        INNER.is_present()
    }

    fn settings_path(&self) -> PathBuf {
        INNER.settings_path()
    }

    fn hook_events(&self) -> Vec<String> {
        INNER.hook_events()
    }

    fn ends_sessions(&self) -> bool {
        INNER.ends_sessions()
    }

    fn hooks_key(&self) -> &str {
        INNER.hooks_key()
    }

    fn hook_file_defaults(&self) -> Map<String, Value> {
        INNER.hook_file_defaults()
    }

    fn hook_entry(&self, event: &str, invocation: &str, native: bool) -> Value {
        INNER.hook_entry(event, invocation, native)
    }

    fn normalize(&self, payload: &Value, event_override: Option<&str>) -> HookEvent {
        let mut e = INNER.normalize(payload, event_override);
        // A stop with background work still running is not the end of the turn.
        if e.event == "Stop" && payload.get("fullyIdle").and_then(Value::as_bool) == Some(false) {
            crate::debug!("agents", "antigravity: Stop with fullyIdle=false read as activity");
            e.event = "PostToolUse".into();
        }
        // Snake-case spellings, should a build send them.
        let s = |k: &str| payload.get(k).and_then(Value::as_str).filter(|v| !v.is_empty()).map(str::to_owned);
        if e.session_id.is_none() {
            e.session_id = s("conversation_id").or_else(|| s("session_id"));
        }
        if e.cwd.is_none() {
            e.cwd = payload.pointer("/workspace_paths/0").and_then(Value::as_str).map(str::to_owned).or_else(|| s("cwd"));
        }
        e
    }

    fn discover_sessions(&self, root: &Path) -> Vec<Session> {
        INNER.discover_sessions(root)
    }

    fn resume_args(&self, session_id: Option<&str>, continue_latest: bool) -> Vec<String> {
        INNER.resume_args(session_id, continue_latest)
    }

    fn matches_process(&self, name: &str, argv: &[String]) -> bool {
        INNER.matches_process(name, argv)
    }

    fn mcp_config(&self) -> Option<McpConfig> {
        INNER.mcp_config()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status::{Status::*, map_event_to_status};
    use serde_json::json;

    fn payload(extra: Value) -> Value {
        let mut p = json!({"conversationId": "ec33ebf9", "workspacePaths": ["/w/repo"],
                           "transcriptPath": "/h/.gemini/antigravity-cli/brain/ec33ebf9/t.jsonl",
                           "artifactDirectoryPath": "/h/a", "modelName": "gemini-3.8-flash"});
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        p
    }

    #[test]
    fn an_antigravity_turn_drives_the_status_rules() {
        let step = |cur, ev: &str, p: Value| map_event_to_status(&ANTIGRAVITY.normalize(&p, Some(ev)), cur, 0);
        let e = ANTIGRAVITY.normalize(&payload(json!({"invocationNum": 0, "initialNumSteps": 1})), Some("PreInvocation"));
        assert_eq!((e.event.as_str(), e.session_id.as_deref(), e.cwd.as_deref()), ("UserPromptSubmit", Some("ec33ebf9"), Some("/w/repo")));
        assert!(e.transcript_path.unwrap().ends_with("t.jsonl"));
        let s = step(None, "SessionStart", payload(json!({})));
        assert_eq!(s, Some(Idle));
        let s = step(s, "PreInvocation", payload(json!({"invocationNum": 0})));
        assert_eq!(s, Some(Executing));
        // A follow-up invocation (after tool results) keeps it executing.
        let s = step(s, "PreInvocation", payload(json!({"invocationNum": 1})));
        assert_eq!(s, Some(Executing));
        // Background work still running: not done yet.
        let s = step(s, "Stop", payload(json!({"executionNum": 1, "terminationReason": "model_stop", "fullyIdle": false})));
        assert_eq!(s, Some(Executing));
        let s = step(s, "Stop", payload(json!({"executionNum": 1, "terminationReason": "model_stop", "fullyIdle": true})));
        assert_eq!(s, Some(Done));
        // Next prompt.
        assert_eq!(step(s, "PreInvocation", payload(json!({"invocationNum": 0}))), Some(Executing));
    }

    #[test]
    fn snake_case_payloads_still_resolve() {
        let e = ANTIGRAVITY.normalize(&json!({"conversation_id": "c1", "workspace_paths": ["/w"]}), Some("Stop"));
        assert_eq!((e.session_id.as_deref(), e.cwd.as_deref()), (Some("c1"), Some("/w")));
        let e = ANTIGRAVITY.normalize(&json!({"session_id": "c2", "cwd": "/x"}), Some("Stop"));
        assert_eq!((e.session_id.as_deref(), e.cwd.as_deref()), (Some("c2"), Some("/x")));
    }

    #[test]
    fn hook_entries_are_flat_in_seconds_under_our_own_group() {
        let e = ANTIGRAVITY.hook_entry("Stop", "\"/bin/am-hook\" hook-event", true);
        assert_eq!(e, json!({"type": "command", "command": "\"/bin/am-hook\" hook-event --agent antigravity --worktree \"\" --event Stop", "timeout": 10}));
        assert_eq!(ANTIGRAVITY.hooks_key(), "agent-monitor");
        assert_eq!(ANTIGRAVITY.hook_events(), ["SessionStart", "PreInvocation", "Stop"]);
        assert!(ANTIGRAVITY.settings_path().ends_with(".gemini/config/hooks.json"));
        assert!(ANTIGRAVITY.hook_file_defaults().is_empty());
    }

    #[test]
    fn has_no_session_end_hook() {
        assert!(!ANTIGRAVITY.ends_sessions(), "am-hook closes settled conversations on SessionStart instead");
        assert!(crate::harness::gemini::GEMINI.ends_sessions());
    }

    #[test]
    fn resumes_a_conversation_and_recognises_agy() {
        assert_eq!(ANTIGRAVITY.program(), "agy");
        assert_eq!(ANTIGRAVITY.resume_args(Some("ec33ebf9"), false), ["--conversation", "ec33ebf9"]);
        assert!(ANTIGRAVITY.resume_args(None, true).is_empty());
        let argv = |s: &str| s.split(' ').map(String::from).collect::<Vec<_>>();
        assert!(ANTIGRAVITY.matches_process("agy", &argv("agy")));
        assert!(ANTIGRAVITY.matches_process("node", &argv("/usr/local/bin/agy --conversation ec33ebf9")));
        // The release tarball ships the binary as `antigravity`.
        assert!(ANTIGRAVITY.matches_process("antigravity", &argv("antigravity")));
        assert!(ANTIGRAVITY.matches_process("node", &argv("/opt/agy/bin/antigravity --conversation ec33ebf9")));
        assert!(!ANTIGRAVITY.matches_process("gemini", &argv("gemini")));
        assert!(!ANTIGRAVITY.matches_process("bash", &argv("bash -c agy-helper")));
    }
}
