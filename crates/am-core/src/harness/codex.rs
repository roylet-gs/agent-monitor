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
//!
//! Context: each turn appends an `event_msg` of type `token_count` whose
//! `info.last_token_usage` is the last request's usage and
//! `info.model_context_window` the model's window, so the bar uses Codex's
//! own window rather than the setting.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use serde_json::Value;

use super::embedded::Embedded;
use super::manifest::Manifest;
use super::{HarnessKind, home_join, is_within};
use crate::context::ContextUsage;
use crate::launch::Session;

pub static CODEX: Embedded =
    Embedded::new(|| Manifest::builtin(include_str!("codex.json")), Some(discover)).with_context(context_usage);

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

/// Rollouts read, so the collector can ask on every sweep: session id →
/// rollout file, and rollout → `(len, mtime)` when read plus what it held.
type Stamp = (u64, SystemTime);
type Rollouts = (HashMap<String, PathBuf>, HashMap<PathBuf, (Stamp, Option<ContextUsage>)>);
static ROLLOUTS: Mutex<Option<Rollouts>> = Mutex::new(None);
/// How much of a rollout's end is read for its last `token_count`.
const TAIL: u64 = 512 * 1024;

fn context_usage(_root: &Path, session_id: &str) -> Option<ContextUsage> {
    let known = ROLLOUTS.lock().ok().and_then(|c| c.as_ref()?.0.get(session_id).cloned()).filter(|p| p.is_file());
    let path = match known {
        Some(p) => p,
        None => {
            let p = rollout_for(&home_join(".codex/sessions"), session_id)?;
            if let Ok(mut c) = ROLLOUTS.lock() {
                c.get_or_insert_with(Default::default).0.insert(session_id.to_owned(), p.clone());
            }
            p
        }
    };
    let meta = std::fs::metadata(&path).ok()?;
    let stamp = (meta.len(), meta.modified().ok()?);
    if let Some((at, usage)) = ROLLOUTS.lock().ok().and_then(|c| c.as_ref()?.1.get(&path).cloned())
        && at == stamp
    {
        return usage;
    }
    let usage = read_rollout_usage(&path);
    if let Ok(mut c) = ROLLOUTS.lock() {
        let c = c.get_or_insert_with(Default::default);
        if c.1.len() > 500 {
            c.1.clear();
        }
        c.1.insert(path, (stamp, usage.clone()));
    }
    usage
}

/// `rollout-<time>-<id>.jsonl` under the newest day directories.
pub fn rollout_for(sessions_dir: &Path, session_id: &str) -> Option<PathBuf> {
    if session_id.is_empty() || session_id.contains('/') {
        return None;
    }
    let suffix = format!("-{session_id}.jsonl");
    day_dirs(sessions_dir).into_iter().take(MAX_DAYS).find_map(|day| {
        std::fs::read_dir(day).ok()?.flatten().map(|f| f.path()).find(|p| {
            p.file_name().map(|n| n.to_string_lossy()).is_some_and(|n| n.starts_with("rollout-") && n.ends_with(&suffix))
        })
    })
}

/// The last `token_count` in a rollout: the context in use (the last
/// request's tokens, less reasoning, which isn't carried into the next
/// turn) and the model's window.
pub fn read_rollout_usage(path: &Path) -> Option<ContextUsage> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let start = len.saturating_sub(TAIL);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = Vec::new();
    file.take(len - start).read_to_end(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf);
    let text = if start > 0 { text.split_once('\n').map_or("", |(_, rest)| rest) } else { &text };
    let mut model = None;
    let mut found = None;
    for line in text.lines().rev() {
        if model.is_none() && line.contains("\"turn_context\"") {
            let v: Value = serde_json::from_str(line).ok().unwrap_or_default();
            model = v.pointer("/payload/model").and_then(Value::as_str).map(str::to_owned);
        }
        if found.is_none() && line.contains("\"token_count\"") {
            let v: Value = serde_json::from_str(line).ok().unwrap_or_default();
            if let Some(info) = v.pointer("/payload/info").filter(|i| !i.is_null()) {
                let last = info.get("last_token_usage")?;
                let n = |k: &str| last.get(k).and_then(Value::as_u64).unwrap_or(0);
                let tokens = n("total_tokens").saturating_sub(n("reasoning_output_tokens"));
                found = Some((tokens, info.get("model_context_window").and_then(Value::as_u64)));
            }
        }
        if found.is_some() && model.is_some() {
            break;
        }
    }
    let (tokens, window) = found?;
    (tokens > 0).then_some(ContextUsage { tokens, model, window })
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

    #[test]
    fn context_comes_from_the_last_token_count_with_codexs_window() {
        let dir = tempfile::tempdir().unwrap();
        let day = dir.path().join("2026/10/13");
        std::fs::create_dir_all(&day).unwrap();
        let count = |total: u64, reasoning: u64| {
            json!({"type": "event_msg", "payload": {"type": "token_count", "info": {
                "last_token_usage": {"input_tokens": total - 900, "cached_input_tokens": 1000, "output_tokens": 900, "reasoning_output_tokens": reasoning, "total_tokens": total},
                "model_context_window": 272000
            }}})
            .to_string()
        };
        let lines = [
            json!({"type": "session_meta", "payload": {"id": "t-9", "cwd": "/w"}}).to_string(),
            json!({"type": "turn_context", "payload": {"model": "gpt-5-codex"}}).to_string(),
            count(40_000, 0),
            json!({"type": "event_msg", "payload": {"type": "token_count", "info": null}}).to_string(),
            count(61_000, 1_000),
        ];
        let file = day.join("rollout-2026-10-13T09-00-00-t-9.jsonl");
        std::fs::write(&file, lines.join("\n")).unwrap();
        assert_eq!(rollout_for(dir.path(), "t-9"), Some(file.clone()));
        assert_eq!(rollout_for(dir.path(), "t-1"), None);
        let u = read_rollout_usage(&file).unwrap();
        assert_eq!((u.tokens, u.window, u.model.as_deref()), (60_000, Some(272_000), Some("gpt-5-codex")));
        assert_eq!(u.window(1_000_000), 272_000, "Codex's window beats the setting");
    }
}
