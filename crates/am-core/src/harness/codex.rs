//! OpenAI Codex CLI as a built-in plugin: the manifest `codex.json` plus
//! session discovery from Codex's rollout files.
//!
//! Codex's hooks (`~/.codex/hooks.json`) use Claude Code's layout, event
//! names and payload keys (`session_id`, `cwd`, `tool_name`…; shell commands
//! arrive as `Bash`, file edits as `apply_patch`). There's no project-dir
//! variable, so am-hook finds the worktree from the payload's `cwd`. Codex
//! only runs a user hook after it has been trusted in `/hooks`, and a changed
//! command needs trusting again. `Interrupt` (the user stopped the turn) ends
//! the turn like `Stop`; there's no `Notification` event, so `waiting` comes
//! from `PermissionRequest`.
//!
//! Sessions: `~/.codex/sessions/YYYY/MM/DD/rollout-<time>-<id>.jsonl`, whose
//! first line is `{"type":"session_meta","payload":{"id","cwd",…}}`.

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::time::SystemTime;

use serde_json::Value;

use super::embedded::Embedded;
use super::manifest::Manifest;
use super::{HarnessKind, home_join, is_within};
use crate::launch::Session;

pub static CODEX: Embedded = Embedded::new(|| Manifest::builtin(include_str!("codex.json")), Some(discover));

/// Day directories scanned, newest first; older sessions aren't offered.
const MAX_DAYS: usize = 60;

fn discover(root: &Path) -> Vec<Session> {
    discover_sessions_in(&home_join(".codex/sessions"), root)
}

/// Sessions started at `root` or below, newest first.
pub fn discover_sessions_in(sessions_dir: &Path, root: &Path) -> Vec<Session> {
    let root = root.to_string_lossy();
    let mut sessions = Vec::new();
    for day in day_dirs(sessions_dir).into_iter().take(MAX_DAYS) {
        let Ok(files) = std::fs::read_dir(&day) else { continue };
        for f in files.flatten() {
            let name = f.file_name().to_string_lossy().into_owned();
            if !name.starts_with("rollout-") || !name.ends_with(".jsonl") {
                continue;
            }
            let Some((id, cwd)) = session_meta(&f.path()) else { continue };
            if !is_within(&cwd, &root) {
                continue;
            }
            let modified = f.metadata().and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
            sessions.push(Session { id, cwd, modified, agent: HarnessKind::CODEX });
        }
    }
    sessions.sort_by_key(|s| std::cmp::Reverse(s.modified));
    sessions
}

/// `YYYY/MM/DD` directories, newest first (the names sort by date).
fn day_dirs(dir: &Path) -> Vec<std::path::PathBuf> {
    let sorted_desc = |d: &Path| {
        let mut v: Vec<_> = std::fs::read_dir(d).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
        v.sort_by(|a, b| b.cmp(a));
        v
    };
    let mut days = Vec::new();
    for year in sorted_desc(dir) {
        for month in sorted_desc(&year) {
            days.extend(sorted_desc(&month));
            if days.len() >= MAX_DAYS {
                return days;
            }
        }
    }
    days
}

/// The rollout's thread id and cwd. Subagent threads (`parent_thread_id`)
/// can't be resumed on their own.
fn session_meta(file: &Path) -> Option<(String, String)> {
    let mut first = String::new();
    BufReader::new(std::fs::File::open(file).ok()?).read_line(&mut first).ok()?;
    let line: Value = serde_json::from_str(first.trim()).ok()?;
    if line.get("type").and_then(Value::as_str) != Some("session_meta") {
        return None;
    }
    let p = line.get("payload")?;
    if p.get("parent_thread_id").is_some_and(|v| !v.is_null()) {
        return None;
    }
    let id = p.get("id")?.as_str().filter(|s| !s.is_empty())?.to_owned();
    let cwd = p.get("cwd")?.as_str()?.to_owned();
    Some((id, cwd))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::Harness;
    use crate::status::{Status::*, map_event_to_status};
    use serde_json::json;

    fn step(cur: Option<crate::status::Status>, p: Value) -> Option<crate::status::Status> {
        map_event_to_status(&CODEX.normalize(&p, None), cur, 0)
    }

    #[test]
    fn a_codex_turn_drives_the_status_rules() {
        let base = |e: &str| json!({"hook_event_name": e, "session_id": "t-1", "cwd": "/w/repo", "model": "gpt-5"});
        let s = step(None, base("SessionStart"));
        assert_eq!(s, Some(Idle));
        let s = step(s, base("UserPromptSubmit"));
        assert_eq!(s, Some(Executing));
        let mut tool = base("PreToolUse");
        tool["tool_name"] = "apply_patch".into();
        let e = CODEX.normalize(&tool, None);
        assert_eq!((e.tool_name.as_deref(), e.cwd.as_deref(), e.session_id.as_deref()), (Some("Edit"), Some("/w/repo"), Some("t-1")));
        assert_eq!(step(s, base("PermissionRequest")), Some(Waiting));
        assert_eq!(step(s, base("Stop")), Some(Done));
        assert_eq!(step(s, base("Interrupt")), Some(Done));
    }

    #[test]
    fn hook_entries_and_resume() {
        let e = CODEX.hook_entry("Stop", "\"/bin/am-hook\" hook-event", true);
        let h = &e["hooks"][0];
        assert_eq!((h["type"].as_str(), h["timeout"].as_u64()), (Some("command"), Some(3)));
        assert_eq!(h["command"], "\"/bin/am-hook\" hook-event --agent codex --worktree \"\" --event Stop");
        assert!(e.get("matcher").is_none());
        assert_eq!(CODEX.hooks_key(), "hooks");
        assert_eq!(CODEX.resume_args(Some("t-1"), false), ["resume", "t-1"]);
        assert_eq!(CODEX.resume_args(None, true), ["resume", "--last"]);
        assert!(CODEX.matches_process("codex", &["codex".into()]));
    }

    #[test]
    fn discovers_rollouts_under_the_root() {
        let dir = tempfile::tempdir().unwrap();
        let mk = |day: &str, file: &str, meta: Value| {
            let d = dir.path().join(day);
            std::fs::create_dir_all(&d).unwrap();
            let line = json!({"timestamp": "t", "type": "session_meta", "payload": meta});
            std::fs::write(d.join(file), format!("{line}\n{{\"type\":\"event_msg\"}}\n")).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(15));
        };
        mk("2026/10/12", "rollout-a.jsonl", json!({"id": "aaa", "cwd": "/w/repo"}));
        mk("2026/10/13", "rollout-b.jsonl", json!({"id": "bbb", "cwd": "/w/repo/pkg"}));
        mk("2026/10/13", "rollout-c.jsonl", json!({"id": "ccc", "cwd": "/w/repo-other"}));
        mk("2026/10/13", "rollout-d.jsonl", json!({"id": "ddd", "cwd": "/w/repo", "parent_thread_id": "aaa"}));
        mk("2026/10/13", "notes.jsonl", json!({"id": "eee", "cwd": "/w/repo"}));
        let s = discover_sessions_in(dir.path(), Path::new("/w/repo"));
        let ids: Vec<&str> = s.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["bbb", "aaa"]);
        assert_eq!(s[0].cwd, "/w/repo/pkg");
        assert!(discover_sessions_in(&dir.path().join("missing"), Path::new("/w/repo")).is_empty());
    }
}
