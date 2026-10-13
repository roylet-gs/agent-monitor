//! Claude Code: hooks in `~/.claude/settings.json`, transcripts in
//! `~/.claude/projects/<encoded cwd>/<session>.jsonl`. Claude's hook payload is
//! am's canonical event format, so `normalize` is the identity mapping.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::{Value, json};

use super::{AgentKind, AgentProvider, ProcessSpec, home_join, is_within};
use crate::launch::Session;
use crate::status::HookEvent;

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

impl AgentProvider for Claude {
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
        crate::context::usage_for(root, session_id)
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
            sessions.push(Session { id, cwd, modified, agent: AgentKind::CLAUDE });
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

#[cfg(test)]
mod tests {
    use super::*;

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
