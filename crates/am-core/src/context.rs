//! How full an agent's context window is, read from the tail of its Claude Code
//! transcript (`~/.claude/projects/<encoded cwd>/<session_id>.jsonl`). Every
//! main-chain assistant message carries the API `usage`; the prompt size of the
//! last one (input + cache read + cache creation) is the context in use.
//!
//! The window itself isn't recorded, and the app doesn't assume any model, so it
//! comes from the `contextWindowTokens` setting, doubled until it holds a session
//! that has gone past it.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::{debug, launch, paths};

/// Smallest `contextWindowTokens` the editors accept.
pub const MIN_WINDOW: u64 = 1_000;
/// Fraction of the window used at which the bar turns yellow, then red.
pub const WARN_AT: f64 = 0.70;
pub const CRITICAL_AT: f64 = 0.90;

/// First tail read; doubled up to `MAX_TAIL` until an assistant turn turns up.
/// Transcripts reach tens of MB, so never read the whole file.
const FIRST_TAIL: u64 = 256 * 1024;
const MAX_TAIL: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextUsage {
    /// Tokens in the context at the last main-chain turn.
    pub tokens: u64,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Ok,
    Warn,
    Critical,
}

/// The window `tokens` are measured against: `configured`, doubled until it
/// holds them (200k → 400k → 800k …), so a session past the setting still gets
/// a meaningful bar whatever model it runs.
pub fn window_for(tokens: u64, configured: u64) -> u64 {
    let mut window = configured.max(1);
    while tokens > window {
        window = window.saturating_mul(2);
    }
    window
}

/// A token count as typed: `128000`, `200k`, `1m`, `1.5M` (`_`/`,` allowed as
/// separators). `None` for anything else, zero included.
pub fn parse_tokens(s: &str) -> Option<u64> {
    let t: String = s.trim().chars().filter(|c| *c != '_' && *c != ',').collect::<String>().to_ascii_lowercase();
    let (num, mult) = match t.strip_suffix('k') {
        Some(n) => (n, 1_000.0),
        None => match t.strip_suffix('m') {
            Some(n) => (n, 1_000_000.0),
            None => (t.as_str(), 1.0),
        },
    };
    let n: f64 = num.trim().parse().ok()?;
    let v = (n * mult).round();
    (1.0..9.0e15).contains(&v).then_some(v as u64)
}

pub fn fraction(tokens: u64, window: u64) -> f64 {
    (tokens as f64 / window.max(1) as f64).clamp(0.0, 1.0)
}

pub fn level(tokens: u64, window: u64) -> Level {
    let f = fraction(tokens, window);
    if f >= CRITICAL_AT {
        Level::Critical
    } else if f >= WARN_AT {
        Level::Warn
    } else {
        Level::Ok
    }
}

/// Compact token count in at most 4 chars: `950`, `12k`, `380k`, `1.2M`, `12M`.
pub fn short_tokens(n: u64) -> String {
    match n {
        0..=999 => n.to_string(),
        1_000..=999_499 => format!("{}k", (n + 500) / 1_000),
        _ => {
            let m = n as f64 / 1_000_000.0;
            if m < 9.95 { format!("{m:.1}M") } else { format!("{}M", m.round() as u64) }
        }
    }
}

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
    let prefix = launch::encode_project_path(&root.to_string_lossy());
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
    fn window_doubles_past_the_setting() {
        assert_eq!(window_for(150_000, 200_000), 200_000);
        assert_eq!(window_for(200_000, 200_000), 200_000);
        assert_eq!(window_for(250_000, 200_000), 400_000);
        assert_eq!(window_for(900_000, 200_000), 1_600_000);
        assert_eq!(window_for(10, 1_000_000), 1_000_000);
        assert_eq!(window_for(5, 0), 8);
        assert_eq!(window_for(u64::MAX, 3), u64::MAX);
    }

    #[test]
    fn parses_typed_token_counts() {
        assert_eq!(parse_tokens("128000"), Some(128_000));
        assert_eq!(parse_tokens("200k"), Some(200_000));
        assert_eq!(parse_tokens(" 1M "), Some(1_000_000));
        assert_eq!(parse_tokens("1.5m"), Some(1_500_000));
        assert_eq!(parse_tokens("1_000_000"), Some(1_000_000));
        assert_eq!(parse_tokens("200,000"), Some(200_000));
        for bad in ["", "abc", "0", "-5", "k", "1g", "1e30"] {
            assert_eq!(parse_tokens(bad), None, "{bad}");
        }
    }

    #[test]
    fn levels_and_short_counts() {
        assert_eq!(level(100_000, 200_000), Level::Ok);
        assert_eq!(level(140_000, 200_000), Level::Warn);
        assert_eq!(level(180_000, 200_000), Level::Critical);
        assert_eq!(level(900_000, 200_000), Level::Critical);
        assert_eq!(short_tokens(950), "950");
        assert_eq!(short_tokens(12_400), "12k");
        assert_eq!(short_tokens(380_200), "380k");
        assert_eq!(short_tokens(999_700), "1.0M");
        assert_eq!(short_tokens(1_240_000), "1.2M");
        assert_eq!(short_tokens(12_000_000), "12M");
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
