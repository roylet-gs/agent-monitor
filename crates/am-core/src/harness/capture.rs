//! Raw hook payloads saved per plugin, so a harness being trained can "listen
//! to itself": `<app dir>/hook-captures/<id>/<unix ms>-<pid>.<seq>-<native event>.json`.
//!
//! `am-hook` writes one for every event of a draft plugin, of an unknown
//! `--agent` id, and of any harness under `AM_HOOK_CAPTURE=1`. Each file keeps
//! the native event name (not the normalised one), the hook's argv and cwd,
//! the environment's variable *names* plus the values of those that point at a
//! directory (how a training agent finds its harness's project-dir variable;
//! nothing else, so no secrets), and the payload. A plugin keeps the newest
//! [`MAX_CAPTURES`]; payloads over [`MAX_PAYLOAD`] bytes are truncated.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value, json};

use crate::paths;

pub const MAX_CAPTURES: usize = 300;
pub const MAX_PAYLOAD: usize = 256 * 1024;

/// One saved event.
#[derive(Debug, Clone)]
pub struct Capture {
    pub path: PathBuf,
    /// Unix milliseconds, from the file name.
    pub received_ms: u128,
    pub native_event: String,
    /// The whole file: `receivedAt`, `nativeEvent`, `argv`, `cwd`, `env`, `payload`, `truncated`.
    pub body: Value,
}

impl Capture {
    pub fn payload(&self) -> &Value {
        self.body.get("payload").unwrap_or(&Value::Null)
    }
}

/// File names carry the event, so keep them to safe characters.
fn file_safe(s: &str) -> String {
    let s: String = s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '_' }).collect();
    if s.is_empty() { "unknown".into() } else { s.chars().take(64).collect() }
}

/// Env var names, and the values of those naming an existing directory.
fn env_snapshot() -> Value {
    let mut names: Vec<String> = Vec::new();
    let mut dirs = Map::new();
    for (k, v) in std::env::vars_os() {
        let Some(k) = k.to_str().map(str::to_owned) else { continue };
        if let Some(v) = v.to_str().filter(|v| v.starts_with('/') && v.len() < 1024 && Path::new(v).is_dir()) {
            dirs.insert(k.clone(), v.into());
        }
        names.push(k);
    }
    names.sort();
    json!({ "names": names, "dirs": dirs })
}

/// Save one event for plugin `id` (already a valid id) and prune the oldest.
/// Best effort: callers ignore the error, hooks never fail.
pub fn write(id: &str, native_event: &str, raw: &str) -> std::io::Result<PathBuf> {
    write_in(&paths::plugin_capture_dir(id), native_event, raw, std::env::args().collect(), env_snapshot())
}

fn write_in(dir: &Path, native_event: &str, raw: &str, argv: Vec<String>, env: Value) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let now = SystemTime::now();
    let ms = now.duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let truncated = raw.len() > MAX_PAYLOAD;
    let payload = if truncated {
        let mut end = MAX_PAYLOAD;
        while !raw.is_char_boundary(end) {
            end -= 1;
        }
        Value::String(raw[..end].to_owned())
    } else {
        serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.to_owned()))
    };
    let received_at = time::OffsetDateTime::from(now).format(&time::format_description::well_known::Rfc3339).unwrap_or_default();
    let body = json!({
        "receivedAt": received_at,
        "nativeEvent": native_event,
        "argv": argv,
        "cwd": std::env::current_dir().map(|d| d.display().to_string()).unwrap_or_default(),
        "env": env,
        "payload": payload,
        "truncated": truncated,
    });
    // pid + a per-process sequence keep same-millisecond events apart.
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = dir.join(format!("{ms:013}-{}.{seq}-{}.json", std::process::id(), file_safe(native_event)));
    std::fs::write(&path, serde_json::to_vec_pretty(&body).map_err(std::io::Error::other)?)?;
    prune(dir, MAX_CAPTURES);
    Ok(path)
}

fn capture_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut files: Vec<PathBuf> =
        entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json")).collect();
    // Zero-padded millisecond prefix: name order is time order.
    files.sort();
    files
}

fn prune(dir: &Path, keep: usize) {
    let files = capture_files(dir);
    if files.len() > keep {
        for f in &files[..files.len() - keep] {
            let _ = std::fs::remove_file(f);
        }
    }
}

fn parse_name(path: &Path) -> Option<(u128, String)> {
    let stem = path.file_stem()?.to_str()?;
    let mut parts = stem.splitn(3, '-');
    let ms = parts.next()?.parse().ok()?;
    let _pid = parts.next()?;
    Some((ms, parts.next()?.to_owned()))
}

/// Every capture of plugin `id`, oldest first.
pub fn list(id: &str) -> Vec<Capture> {
    list_in(&paths::plugin_capture_dir(id))
}

pub fn list_in(dir: &Path) -> Vec<Capture> {
    capture_files(dir).into_iter().filter_map(|p| read(&p)).collect()
}

/// Read one capture file (also accepts a bare payload file, e.g. an old
/// `AM_HOOK_CAPTURE` capture or a hand-written fixture).
pub fn read(path: &Path) -> Option<Capture> {
    let value: Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let (received_ms, name_event) = parse_name(path).unwrap_or((0, String::new()));
    let (native_event, body) = match value.get("payload") {
        Some(_) if value.get("nativeEvent").is_some() => {
            (value.get("nativeEvent").and_then(Value::as_str).unwrap_or("unknown").to_owned(), value)
        }
        _ => {
            let ev = value.get("hook_event_name").and_then(Value::as_str).map(str::to_owned).unwrap_or(name_event);
            (ev.clone(), json!({ "nativeEvent": ev, "payload": value }))
        }
    };
    Some(Capture { path: path.to_path_buf(), received_ms, native_event, body })
}

/// Delete plugin `id`'s captures; returns how many.
pub fn clear(id: &str) -> usize {
    let files = capture_files(&paths::plugin_capture_dir(id));
    files.iter().filter(|f| std::fs::remove_file(f).is_ok()).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_native_events_and_keeps_a_bounded_ring() {
        let dir = tempfile::tempdir().unwrap();
        let env = json!({ "names": ["HOME"], "dirs": {} });
        for i in 0..5 {
            write_in(dir.path(), "Tool Start", &format!(r#"{{"n":{i},"session_id":"s"}}"#), vec!["am-hook".into()], env.clone()).unwrap();
        }
        prune(dir.path(), 3);
        let caps = list_in(dir.path());
        assert_eq!(caps.len(), 3);
        assert_eq!(caps[0].native_event, "Tool Start");
        assert!(caps[0].path.file_name().unwrap().to_str().unwrap().ends_with("-Tool_Start.json"));
        assert_eq!(caps.last().unwrap().payload()["n"], 4);
        assert_eq!(caps[0].body["argv"][0], "am-hook");
    }

    #[test]
    fn oversized_and_non_json_payloads_are_kept_as_text() {
        let dir = tempfile::tempdir().unwrap();
        let big = "x".repeat(MAX_PAYLOAD + 10);
        let p = write_in(dir.path(), "E", &big, vec![], Value::Null).unwrap();
        let c = read(&p).unwrap();
        assert_eq!(c.body["truncated"], true);
        assert_eq!(c.payload().as_str().unwrap().len(), MAX_PAYLOAD);
        let p = write_in(dir.path(), "E", "not json", vec![], Value::Null).unwrap();
        assert_eq!(read(&p).unwrap().payload(), "not json");
    }

    #[test]
    fn env_snapshot_keeps_names_and_only_directory_values() {
        let env = env_snapshot();
        let names: Vec<&str> = env["names"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
        assert!(names.contains(&"PATH"));
        for (k, v) in env["dirs"].as_object().unwrap() {
            assert!(Path::new(v.as_str().unwrap()).is_dir(), "{k}");
        }
        assert!(env["dirs"].get("PATH").is_none());
    }

    #[test]
    fn bare_payload_files_read_as_captures() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("fixture.json");
        std::fs::write(&p, r#"{"hook_event_name":"TurnDone","session_id":"s"}"#).unwrap();
        let c = read(&p).unwrap();
        assert_eq!(c.native_event, "TurnDone");
        assert_eq!(c.payload()["session_id"], "s");
    }
}
