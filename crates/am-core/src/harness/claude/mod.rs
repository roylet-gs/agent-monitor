//! Claude Code: hooks in `~/.claude/settings.json`, transcripts in
//! `~/.claude/projects/<encoded cwd>/<session>.jsonl`. Claude's hook payload is
//! am's canonical event format, so `normalize` is the identity mapping.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::{Value, json};

use super::{Harness, HarnessKind, ProcessSpec, home_join, is_within};
use crate::launch::Session;
use crate::status::HookEvent;

pub mod transcript;

pub const HOOK_EVENTS: [&str; 11] = [
    "PreToolUse",
    "PostToolUse",
    "Stop",
    "StopFailure",
    "Notification",
    "SessionStart",
    "SessionEnd",
    "UserPromptSubmit",
    "SubagentStart",
    "SubagentStop",
    "PermissionRequest",
];

/// Events Claude Code rewrites to `--managed` with a 5-minute timeout; writing
/// them in that shape keeps install idempotent.
const MANAGED_HOOK_EVENTS: [&str; 1] = ["PermissionRequest"];

/// Appended to native hook commands; see [`Claude::hook_entry`].
pub const NATIVE_COMPAT_SUFFIX: &str = " # am hook-event (native)";

pub struct Claude;

impl Harness for Claude {
    fn id(&self) -> &str {
        "claude"
    }

    fn name(&self) -> &str {
        "Claude Code"
    }

    fn short_name(&self) -> &str {
        "Claude"
    }

    fn program(&self) -> &str {
        "claude"
    }

    fn is_present(&self) -> bool {
        home_join(".claude").is_dir()
    }

    fn settings_path(&self) -> PathBuf {
        home_join(".claude").join("settings.json")
    }

    fn hook_events(&self) -> Vec<String> {
        HOOK_EVENTS.iter().map(|e| e.to_string()).collect()
    }

    fn hook_entry(&self, event: &str, invocation: &str, native: bool) -> Value {
        let managed = MANAGED_HOOK_EVENTS.contains(&event);
        let managed_flag = if managed { " --managed" } else { "" };
        // Old TS releases (≤1.0.95) only recognise their hooks by the literal
        // text `am hook-event`; without it they think hooks are missing and add a
        // second, TS handler that races this one and overwrites `delegating`.
        // A trailing shell comment keeps them seeing the native hook as "ours".
        let compat = if native { NATIVE_COMPAT_SUFFIX } else { "" };
        json!({
            "hooks": [{
                "type": "command",
                "command": format!("{invocation} --worktree \"$CLAUDE_PROJECT_DIR\" --event {event}{managed_flag}{compat}"),
                "timeout": if managed { 300_000 } else { 5_000 },
            }]
        })
    }

    fn normalize(&self, payload: &Value, event_override: Option<&str>) -> HookEvent {
        HookEvent::from_json(payload, event_override)
    }

    fn discover_sessions(&self, root: &Path) -> Vec<Session> {
        discover_sessions_in(&home_join(".claude").join("projects"), root)
    }

    fn resume_args(&self, session_id: Option<&str>, continue_latest: bool) -> Vec<String> {
        match (session_id, continue_latest) {
            (Some(id), _) => vec!["--resume".into(), id.into()],
            (None, true) => vec!["-c".into()],
            (None, false) => vec![],
        }
    }

    fn context_usage(&self, root: &Path, session_id: &str) -> Option<crate::context::ContextUsage> {
        transcript::usage_for(root, session_id)
    }

    fn usage_files(&self, since: SystemTime) -> Vec<PathBuf> {
        usage_files_in(&home_join(".claude").join("projects"), since)
    }

    fn usage_record(&self, line: &str) -> Option<crate::usage::UsageRecord> {
        usage_record(line)
    }

    fn matches_process(&self, name: &str, argv: &[String]) -> bool {
        static SPEC: std::sync::LazyLock<ProcessSpec> = std::sync::LazyLock::new(|| ProcessSpec {
            names: vec!["claude".into()],
            argv_contains: vec!["/@anthropic-ai/claude-code/".into()],
        });
        SPEC.matches(name, argv)
    }
}

/// Claude Code's project-dir name for a cwd: every non-alphanumeric → `-`.
pub fn encode_project_path(cwd: &str) -> String {
    cwd.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

pub fn discover_sessions_in(projects_dir: &Path, root: &Path) -> Vec<Session> {
    let root = root.to_string_lossy();
    let prefix = encode_project_path(&root);
    let Ok(dirs) = std::fs::read_dir(projects_dir) else { return Vec::new() };
    let mut sessions = Vec::new();
    for dir in dirs.flatten() {
        let name = dir.file_name().to_string_lossy().into_owned();
        // Prefix match is only a cheap pre-filter: `-` also encodes `.` and `/`,
        // so the real cwd inside the transcript is what decides.
        if name != prefix && !name.starts_with(&format!("{prefix}-")) {
            continue;
        }
        let Ok(files) = std::fs::read_dir(dir.path()) else { continue };
        for f in files.flatten() {
            let path = f.path();
            if path.extension().is_none_or(|e| e != "jsonl") {
                continue;
            }
            let Some(cwd) = transcript_cwd(&path) else { continue };
            if !is_within(&cwd, &root) {
                continue;
            }
            let modified = f.metadata().and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
            let id = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
            sessions.push(Session { id, cwd, modified, agent: HarnessKind::CLAUDE });
        }
    }
    sessions.sort_by_key(|s| std::cmp::Reverse(s.modified));
    sessions
}

/// First `cwd` stamped in a transcript (within the first 50 lines).
fn transcript_cwd(file: &Path) -> Option<String> {
    let reader = BufReader::new(std::fs::File::open(file).ok()?);
    reader.lines().take(50).map_while(Result::ok).find_map(|line| {
        let v: Value = serde_json::from_str(&line).ok()?;
        v.get("cwd")?.as_str().filter(|c| !c.is_empty()).map(str::to_owned)
    })
}

/// `<projects>/*/*.jsonl` and `<projects>/*/<session>/subagents/*.jsonl`
/// modified at or after `since`.
pub fn usage_files_in(projects_dir: &Path, since: SystemTime) -> Vec<PathBuf> {
    let fresh = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).is_ok_and(|t| t >= since);
    let jsonl = |dir: &Path| -> Vec<PathBuf> {
        std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "jsonl") && fresh(p))
            .collect()
    };
    let mut out = Vec::new();
    for project in std::fs::read_dir(projects_dir).into_iter().flatten().flatten() {
        let project = project.path();
        // A directory's mtime only moves when entries are added, not when a
        // transcript grows, so every file is checked on its own.
        for entry in std::fs::read_dir(&project).into_iter().flatten().flatten() {
            let path = entry.path();
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                let subagents = path.join("subagents");
                if subagents.is_dir() {
                    out.extend(jsonl(&subagents));
                }
            } else if path.extension().is_some_and(|x| x == "jsonl") && fresh(&path) {
                out.push(path);
            }
        }
    }
    out
}

/// Token usage on an assistant line: input + cache creation + cache read +
/// output. Sidechain (subagent) turns count; `<synthetic>` ones don't.
pub fn usage_record(line: &str) -> Option<crate::usage::UsageRecord> {
    if !line.contains("\"usage\"") || !line.contains("\"assistant\"") {
        return None;
    }
    let v: Value = serde_json::from_str(line).ok()?;
    if v.get("type")?.as_str()? != "assistant" {
        return None;
    }
    let msg = v.get("message")?;
    if msg.get("model").and_then(Value::as_str) == Some("<synthetic>") {
        return None;
    }
    let usage = msg.get("usage")?;
    let n = |k: &str| usage.get(k).and_then(Value::as_u64).unwrap_or(0);
    let tokens = n("input_tokens") + n("cache_creation_input_tokens") + n("cache_read_input_tokens") + n("output_tokens");
    let id = [msg.get("id"), v.get("requestId"), v.get("uuid")].into_iter().flatten().find_map(Value::as_str)?;
    Some(crate::usage::UsageRecord {
        id: id.to_owned(),
        at: crate::timefmt::parse_iso(v.get("timestamp")?.as_str()?)?,
        cwd: v.get("cwd").and_then(Value::as_str).filter(|c| !c.is_empty()).map(str::to_owned),
        tokens,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_records_count_every_token_class() {
        let line = r#"{"type":"assistant","isSidechain":true,"cwd":"/w","timestamp":"1970-01-01T00:01:00.5Z","message":{"id":"msg_1","model":"m","usage":{"input_tokens":2,"cache_creation_input_tokens":30,"cache_read_input_tokens":400,"output_tokens":5000}}}"#;
        let r = usage_record(line).unwrap();
        assert_eq!((r.id.as_str(), r.at, r.cwd.as_deref(), r.tokens), ("msg_1", 60, Some("/w"), 5432));
        let synthetic = line.replace(r#""model":"m""#, r#""model":"<synthetic>""#);
        assert_eq!(usage_record(&synthetic), None);
        let user = line.replace(r#""type":"assistant""#, r#""type":"user""#);
        assert_eq!(usage_record(&user), None);
        assert_eq!(usage_record(r#"{"type":"summary"}"#), None);
    }

    #[test]
    fn usage_files_include_subagents_and_skip_stale() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("-w");
        std::fs::create_dir_all(project.join("s1/subagents")).unwrap();
        for f in ["s1.jsonl", "s1/subagents/agent-a.jsonl", "notes.txt"] {
            std::fs::write(project.join(f), "").unwrap();
        }
        let mut files = usage_files_in(dir.path(), SystemTime::UNIX_EPOCH);
        files.sort();
        assert_eq!(files, vec![project.join("s1/subagents/agent-a.jsonl"), project.join("s1.jsonl")]);
        let later = SystemTime::now() + std::time::Duration::from_secs(60);
        assert!(usage_files_in(dir.path(), later).is_empty());
    }

    #[test]
    fn entry_shape_matches_ts() {
        let e = Claude.hook_entry("PermissionRequest", "\"/bin/am-hook\" hook-event", true);
        assert_eq!(
            e["hooks"][0]["command"],
            "\"/bin/am-hook\" hook-event --worktree \"$CLAUDE_PROJECT_DIR\" --event PermissionRequest --managed # am hook-event (native)"
        );
        assert_eq!(e["hooks"][0]["timeout"], 300_000);
        let e = Claude.hook_entry("Stop", "cat | am hook-event", false);
        assert_eq!(e["hooks"][0]["command"], "cat | am hook-event --worktree \"$CLAUDE_PROJECT_DIR\" --event Stop");
        assert_eq!(e["hooks"][0]["timeout"], 5_000);
    }

    #[test]
    fn resume_args() {
        assert_eq!(Claude.resume_args(Some("abc"), true), ["--resume", "abc"]);
        assert_eq!(Claude.resume_args(None, true), ["-c"]);
        assert!(Claude.resume_args(None, false).is_empty());
    }

    #[test]
    fn encodes_like_claude_code() {
        assert_eq!(encode_project_path("/Users/a/x.y/.claude/worktrees/f"), "-Users-a-x-y--claude-worktrees-f");
    }

    #[test]
    fn discovers_sessions_at_root_and_subdirs_newest_first() {
        let projects = tempfile::tempdir().unwrap();
        let root = "/w/repo";
        let mk = |dir: &str, id: &str, cwd: &str| {
            let d = projects.path().join(dir);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join(format!("{id}.jsonl")), format!("{{\"type\":\"x\"}}\n{{\"cwd\":\"{cwd}\"}}\n")).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(15));
        };
        mk("-w-repo", "old", "/w/repo");
        mk("-w-repo-pkg", "sub", "/w/repo/pkg");
        mk("-w-repo-other", "sibling", "/w/repo-other"); // prefix collision, other dir
        mk("-w-elsewhere", "nope", "/w/elsewhere");
        let s = discover_sessions_in(projects.path(), Path::new(root));
        let ids: Vec<&str> = s.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["sub", "old"]);
        assert_eq!(s[0].cwd, "/w/repo/pkg");
    }
}
