//! Gemini CLI (`@google/gemini-cli`), as a built-in plugin: everything but
//! session discovery is the declarative manifest `gemini.json` (the same
//! format users drop into `<app dir>/agents/`), which shows the format covers
//! a real agent. Sessions need code: Gemini keeps them in
//! `~/.gemini/tmp/<project slug>/chats/session-*.jsonl`, with the slug looked
//! up in `~/.gemini/projects.json`.
//!
//! Gemini's hook system is modelled on Claude Code's (same `hooks` shape, same
//! stdin JSON keys for the common fields); the manifest maps its event names
//! (`BeforeAgent` → `UserPromptSubmit`, `AfterAgent` → `Stop`, `BeforeTool` /
//! `AfterTool` → `PreToolUse` / `PostToolUse`, `ToolPermission` notifications →
//! `permission_prompt`) and its built-in tool names onto Claude's.
//!
//! Gemini has no subagent lifecycle events and tags no hook with an agent id:
//! a subagent is an ordinary `invoke_agent` tool call, so the main agent stays
//! `executing` while it runs and nothing is recorded in `subagents`.
//! Hooks only run in folders Gemini trusts (`folderTrust`), user-level ones
//! included. `BeforeModel`, `AfterModel` (per streamed chunk),
//! `BeforeToolSelection` and `PreCompress` carry no status and aren't hooked.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::SystemTime;

use serde_json::Value;

use super::manifest::{Manifest, expand_home};
use super::{Harness, HarnessKind, home_join, is_within};
use crate::launch::Session;
use crate::status::HookEvent;

static MANIFEST: LazyLock<Manifest> =
    LazyLock::new(|| Manifest::parse(include_str!("gemini.json")).expect("built-in gemini.json is valid"));

pub struct Gemini;

impl Harness for Gemini {
    fn id(&self) -> &str {
        &MANIFEST.id
    }

    fn name(&self) -> &str {
        &MANIFEST.name
    }

    fn short_name(&self) -> &str {
        MANIFEST.short_name.as_deref().unwrap_or(&MANIFEST.name)
    }

    fn program(&self) -> &str {
        &MANIFEST.program
    }

    fn notes(&self) -> Option<&str> {
        MANIFEST.notes.as_deref()
    }

    fn is_present(&self) -> bool {
        gemini_dir().is_dir()
    }

    fn settings_path(&self) -> PathBuf {
        expand_home(&MANIFEST.hooks.settings_path)
    }

    fn hook_events(&self) -> Vec<String> {
        MANIFEST.hook_events()
    }

    fn hook_entry(&self, event: &str, invocation: &str, _native: bool) -> Value {
        MANIFEST.hook_entry(event, invocation)
    }

    fn normalize(&self, payload: &Value, event_override: Option<&str>) -> HookEvent {
        MANIFEST.normalize(payload, event_override)
    }

    fn discover_sessions(&self, root: &Path) -> Vec<Session> {
        discover_sessions_in(&gemini_dir(), root)
    }

    fn resume_args(&self, session_id: Option<&str>, continue_latest: bool) -> Vec<String> {
        MANIFEST.resume_args(session_id, continue_latest)
    }

    fn matches_process(&self, name: &str, argv: &[String]) -> bool {
        MANIFEST.process.matches(name, argv)
    }
}

pub fn normalize(v: &Value, event_override: Option<&str>) -> HookEvent {
    MANIFEST.normalize(v, event_override)
}

fn gemini_dir() -> PathBuf {
    home_join(".gemini")
}

/// Sessions for `root` and its subdirectories. Gemini keys a project by the
/// directory it was launched in (not the git root), so every registered
/// project path inside `root` is searched.
pub fn discover_sessions_in(gemini_dir: &Path, root: &Path) -> Vec<Session> {
    let root = root.to_string_lossy();
    let Some(projects) = std::fs::read_to_string(gemini_dir.join("projects.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v.get("projects").and_then(Value::as_object).cloned())
    else {
        return Vec::new();
    };
    let mut sessions = Vec::new();
    for (project, slug) in &projects {
        let Some(slug) = slug.as_str() else { continue };
        // Slugs are `[a-z0-9-]+`; anything else could escape the tmp dir.
        if !is_within(project, &root) || slug.is_empty() || !slug.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            continue;
        }
        let Ok(files) = std::fs::read_dir(gemini_dir.join("tmp").join(slug).join("chats")) else { continue };
        for f in files.flatten() {
            let path = f.path();
            let name = f.file_name().to_string_lossy().into_owned();
            if !name.starts_with("session-") || !name.ends_with(".jsonl") {
                continue;
            }
            let Some(id) = main_session_id(&path) else { continue };
            let modified = f.metadata().and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
            sessions.push(Session { id, cwd: project.clone(), modified, agent: HarnessKind::GEMINI });
        }
    }
    sessions.sort_by_key(|s| std::cmp::Reverse(s.modified));
    sessions
}

/// The first JSONL record is the session metadata; subagent transcripts
/// (`kind: "subagent"`) can't be resumed on their own.
fn main_session_id(file: &Path) -> Option<String> {
    let mut first = String::new();
    BufReader::new(std::fs::File::open(file).ok()?).read_line(&mut first).ok()?;
    let meta: Value = serde_json::from_str(first.trim()).ok()?;
    if meta.get("kind").and_then(Value::as_str) == Some("subagent") {
        return None;
    }
    meta.get("sessionId")?.as_str().filter(|s| !s.is_empty()).map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status::{Status, map_event_to_status};
    use serde_json::json;

    fn ev(payload: Value) -> HookEvent {
        normalize(&payload, None)
    }

    #[test]
    fn maps_events_and_fields() {
        let e = ev(json!({"hook_event_name":"BeforeAgent","session_id":"s1","cwd":"/w","transcript_path":"","prompt":"hi"}));
        assert_eq!(e.event, "UserPromptSubmit");
        assert_eq!(e.session_id.as_deref(), Some("s1"));
        assert_eq!(e.transcript_path, None);

        let e = ev(json!({"hook_event_name":"AfterAgent","prompt_response":"All done.","stop_hook_active":false}));
        assert_eq!(e.event, "Stop");
        assert_eq!(e.last_assistant_message.as_deref(), Some("All done."));

        let e = ev(json!({"hook_event_name":"BeforeTool","tool_name":"run_shell_command","tool_input":{"command":"ls"}}));
        assert_eq!((e.event.as_str(), e.tool_name.as_deref()), ("PreToolUse", Some("Bash")));

        let e = ev(json!({"hook_event_name":"AfterTool","tool_name":"mcp_github_list_prs"}));
        assert_eq!((e.event.as_str(), e.tool_name.as_deref()), ("PostToolUse", Some("mcp_github_list_prs")));

        let e = ev(json!({"hook_event_name":"Notification","notification_type":"ToolPermission","message":"Allow?"}));
        assert_eq!(e.notification_type.as_deref(), Some("permission_prompt"));
        assert!(e.needs_user());

        // The --event flag wins over the payload, like Claude's.
        assert_eq!(normalize(&json!({"hook_event_name":"x"}), Some("AfterTool")).event, "PostToolUse");
        // Gemini never tags subagents, so nothing is treated as subagent-originated.
        assert!(!ev(json!({"hook_event_name":"BeforeTool","tool_name":"invoke_agent"})).from_subagent());
    }

    #[test]
    fn a_gemini_turn_drives_the_status_rules() {
        use Status::*;
        let step = |cur: Option<Status>, p: Value| map_event_to_status(&ev(p), cur, 0);
        let s = step(None, json!({"hook_event_name":"SessionStart","source":"startup"}));
        assert_eq!(s, Some(Idle));
        let s = step(s, json!({"hook_event_name":"BeforeAgent","prompt":"fix it"}));
        assert_eq!(s, Some(Executing));
        let s = step(s, json!({"hook_event_name":"Notification","notification_type":"ToolPermission"}));
        assert_eq!(s, Some(Waiting));
        let s = step(s, json!({"hook_event_name":"BeforeTool","tool_name":"replace"}));
        assert_eq!(s, Some(Executing));
        let s = step(s, json!({"hook_event_name":"AfterTool","tool_name":"replace"}));
        assert_eq!(s, Some(Executing));
        let s = step(s, json!({"hook_event_name":"AfterAgent","prompt_response":"done"}));
        assert_eq!(s, Some(Done));
        assert_eq!(step(Some(Executing), json!({"hook_event_name":"BeforeTool","tool_name":"ask_user"})), Some(Waiting));
        assert_eq!(step(Some(Executing), json!({"hook_event_name":"BeforeTool","tool_name":"enter_plan_mode"})), Some(Planning));
        assert_eq!(step(None, json!({"hook_event_name":"SessionEnd","reason":"exit"})), Some(Idle));
    }

    #[test]
    fn hook_entry_shape() {
        let e = Gemini.hook_entry("BeforeTool", "\"/bin/am-hook\" hook-event", true);
        assert_eq!(e["matcher"], "*");
        assert_eq!(e["hooks"][0]["name"], "agent-monitor");
        assert_eq!(e["hooks"][0]["timeout"], 5_000);
        let cmd = e["hooks"][0]["command"].as_str().unwrap();
        assert_eq!(cmd, "\"/bin/am-hook\" hook-event --agent gemini --worktree \"$GEMINI_PROJECT_DIR\" --event BeforeTool");
        assert!(crate::hooks_installer::is_am_hook_command(cmd));
    }

    #[test]
    fn resume_args() {
        assert_eq!(Gemini.resume_args(Some("u-1"), false), ["--resume", "u-1"]);
        assert_eq!(Gemini.resume_args(None, true), ["--resume", "latest"]);
        assert!(Gemini.resume_args(None, false).is_empty());
    }

    #[test]
    fn discovers_sessions_via_the_project_registry() {
        let home = tempfile::tempdir().unwrap();
        let g = home.path();
        std::fs::write(
            g.join("projects.json"),
            r#"{"projects":{"/w/repo":"repo","/w/repo/pkg":"pkg","/w/repo-other":"repo-other","/w/x":"../escape"}}"#,
        )
        .unwrap();
        let mk = |slug: &str, file: &str, first: &str| {
            let d = g.join("tmp").join(slug).join("chats");
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join(file), format!("{first}\n{{\"id\":\"m1\",\"type\":\"user\"}}\n")).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(15));
        };
        mk("repo", "session-2026-10-01T10-00-aaaa1111.jsonl", r#"{"sessionId":"aaaa1111-full","projectHash":"h","kind":"main"}"#);
        mk("pkg", "session-2026-10-02T10-00-bbbb2222.jsonl", r#"{"sessionId":"bbbb2222-full","projectHash":"h"}"#);
        mk("repo", "session-2026-10-03T10-00-cccc3333.jsonl", r#"{"sessionId":"cccc3333-full","kind":"subagent"}"#);
        mk("repo", "logs.json", r#"{"sessionId":"nope"}"#);
        mk("repo-other", "session-2026-10-04T10-00-dddd4444.jsonl", r#"{"sessionId":"dddd4444-full"}"#);
        let s = discover_sessions_in(g, Path::new("/w/repo"));
        let ids: Vec<&str> = s.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["bbbb2222-full", "aaaa1111-full"]);
        assert_eq!(s[0].cwd, "/w/repo/pkg");
        assert!(s.iter().all(|s| s.agent == HarnessKind::GEMINI));
        assert!(discover_sessions_in(&g.join("missing"), Path::new("/w/repo")).is_empty());
    }
}
