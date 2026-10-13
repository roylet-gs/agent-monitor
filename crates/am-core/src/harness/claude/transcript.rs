//! Claude Code's context usage (`Harness::context_usage` for the built-in
//! Claude plugin), read from the tail of its transcript
//! (`~/.claude/projects/<encoded cwd>/<session_id>.jsonl`). Every main-chain
//! assistant message carries the API `usage`; the prompt size of the last one
//! (input + cache read + cache creation) is the context in use.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::encode_project_path;
use crate::context::ContextUsage;
use crate::{debug, paths};

/// First tail read; doubled up to `MAX_TAIL` until an assistant turn turns up.
/// Transcripts reach tens of MB, so never read the whole file.
const FIRST_TAIL: u64 = 256 * 1024;
const MAX_TAIL: u64 = 4 * 1024 * 1024;

/// Context usage for the session `session_id` of the worktree at `root`.
pub fn usage_for(root: &Path, session_id: &str) -> Option<ContextUsage> {
    let path = transcript_path(root, session_id)?;
    let usage = read_usage(&path);
    if usage.is_none() {
        debug!("context", "no usage in {}", path.display());
    }
    usage
}

/// `<projects>/<encoded root>/<id>.jsonl`, else the same file under a
/// subdirectory's project dir (a session started below the worktree root).
pub fn transcript_path(root: &Path, session_id: &str) -> Option<PathBuf> {
    transcript_path_in(&paths::home_dir().join(".claude").join("projects"), root, session_id)
}

pub fn transcript_path_in(projects_dir: &Path, root: &Path, session_id: &str) -> Option<PathBuf> {
    if session_id.is_empty() || session_id.contains('/') {
        return None;
    }
    let file = format!("{session_id}.jsonl");
    let prefix = encode_project_path(&root.to_string_lossy());
    let exact = projects_dir.join(&prefix).join(&file);
    if exact.is_file() {
        return Some(exact);
    }
    let sub = format!("{prefix}-");
    std::fs::read_dir(projects_dir).ok()?.flatten().find_map(|d| {
        let name = d.file_name();
        let p = d.path().join(&file);
        (name.to_string_lossy().starts_with(&sub) && p.is_file()).then_some(p)
    })
}

/// Usage of the last main-chain assistant turn in a transcript. `None` when
/// there is none yet, or when the context was compacted after it (the next
/// turn reports the new size).
pub fn read_usage(path: &Path) -> Option<ContextUsage> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let mut tail = FIRST_TAIL;
    loop {
        let start = len.saturating_sub(tail);
        file.seek(SeekFrom::Start(start)).ok()?;
        let mut buf = Vec::with_capacity((len - start) as usize);
        file.by_ref().take(len - start).read_to_end(&mut buf).ok()?;
        let text = String::from_utf8_lossy(&buf);
        // A tail that starts mid-file starts mid-line: drop the fragment.
        let text = if start > 0 { text.split_once('\n').map_or("", |(_, rest)| rest) } else { &text };
        if let Some(found) = scan(text) {
            return found;
        }
        if start == 0 || tail >= MAX_TAIL {
            return None;
        }
        tail *= 2;
    }
}

/// `Some(result)` once the scan is conclusive, `None` to read further back.
fn scan(text: &str) -> Option<Option<ContextUsage>> {
    for line in text.lines().rev() {
        if line.contains("\"compact_boundary\"") {
            let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
            if v.get("subtype").and_then(Value::as_str) == Some("compact_boundary") {
                return Some(None);
            }
        }
        if !line.contains("\"assistant\"") || !line.contains("\"usage\"") {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        if let Some(u) = assistant_usage(&v) {
            return Some(Some(u));
        }
    }
    None
}

fn assistant_usage(v: &Value) -> Option<ContextUsage> {
    if v.get("type").and_then(Value::as_str) != Some("assistant") {
        return None;
    }
    if v.get("isSidechain").and_then(Value::as_bool).unwrap_or(false) {
        return None;
    }
    let msg = v.get("message")?;
    let model = msg.get("model").and_then(Value::as_str).map(str::to_owned);
    if model.as_deref() == Some("<synthetic>") {
        return None;
    }
    let usage = msg.get("usage")?;
    let n = |k: &str| usage.get(k).and_then(Value::as_u64).unwrap_or(0);
    let tokens = n("input_tokens") + n("cache_read_input_tokens") + n("cache_creation_input_tokens");
    (tokens > 0).then_some(ContextUsage { tokens, model })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assistant(input: u64, read: u64, create: u64) -> String {
        format!(
            r#"{{"type":"assistant","isSidechain":false,"message":{{"model":"claude-opus-5-5","usage":{{"input_tokens":{input},"cache_read_input_tokens":{read},"cache_creation_input_tokens":{create},"output_tokens":50}}}}}}"#
        )
    }

    fn write(dir: &Path, name: &str, lines: &[String]) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, lines.join("\n") + "\n").unwrap();
        p
    }

    #[test]
    fn last_main_chain_turn_wins_and_sums_the_inputs() {
        let dir = tempfile::tempdir().unwrap();
        let side = r#"{"type":"assistant","isSidechain":true,"message":{"model":"m","usage":{"input_tokens":999999}}}"#;
        let synthetic = r#"{"type":"assistant","message":{"model":"<synthetic>","usage":{"input_tokens":0}}}"#;
        let p = write(
            dir.path(),
            "s.jsonl",
            &[
                assistant(1, 10, 100),
                r#"{"type":"user","message":{"content":"hi"}}"#.into(),
                assistant(2, 60_000, 1_500),
                side.into(),
                synthetic.into(),
            ],
        );
        let u = read_usage(&p).unwrap();
        assert_eq!(u.tokens, 61_502);
        assert_eq!(u.model.as_deref(), Some("claude-opus-5-5"));
    }

    #[test]
    fn compaction_after_the_last_turn_clears_it() {
        let dir = tempfile::tempdir().unwrap();
        let boundary = r#"{"type":"system","subtype":"compact_boundary","content":"Conversation compacted"}"#;
        let p = write(dir.path(), "s.jsonl", &[assistant(1, 300_000, 0), boundary.into()]);
        assert_eq!(read_usage(&p), None);
        let p = write(dir.path(), "t.jsonl", &[assistant(1, 300_000, 0), boundary.into(), assistant(1, 20_000, 0)]);
        assert_eq!(read_usage(&p).unwrap().tokens, 20_001);
    }

    #[test]
    fn reads_only_the_tail_of_a_big_file() {
        let dir = tempfile::tempdir().unwrap();
        let filler = format!(r#"{{"type":"user","message":{{"content":"{}"}}}}"#, "x".repeat(1000));
        let mut lines = vec![assistant(1, 1, 1)];
        lines.extend(std::iter::repeat_n(filler, 600)); // ~600 KB: needs a second, wider read
        let p = write(dir.path(), "big.jsonl", &lines);
        assert_eq!(read_usage(&p).unwrap().tokens, 3);
        lines.push(assistant(5, 5, 5));
        let p = write(dir.path(), "big2.jsonl", &lines);
        assert_eq!(read_usage(&p).unwrap().tokens, 15);
    }

    #[test]
    fn missing_or_empty_transcripts_have_no_usage() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read_usage(&dir.path().join("nope.jsonl")), None);
        let p = write(dir.path(), "e.jsonl", &[r#"{"type":"user"}"#.into()]);
        assert_eq!(read_usage(&p), None);
    }

    #[test]
    fn finds_the_transcript_at_the_root_or_a_subdir() {
        let projects = tempfile::tempdir().unwrap();
        let root = Path::new("/w/repo");
        let exact = projects.path().join("-w-repo");
        let sub = projects.path().join("-w-repo-crates");
        std::fs::create_dir_all(&exact).unwrap();
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(exact.join("a.jsonl"), "").unwrap();
        std::fs::write(sub.join("b.jsonl"), "").unwrap();
        assert_eq!(transcript_path_in(projects.path(), root, "a"), Some(exact.join("a.jsonl")));
        assert_eq!(transcript_path_in(projects.path(), root, "b"), Some(sub.join("b.jsonl")));
        assert_eq!(transcript_path_in(projects.path(), root, "c"), None);
        assert_eq!(transcript_path_in(projects.path(), root, "../x"), None);
    }
}
