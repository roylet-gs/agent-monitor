//! The anonymous usage and error events: the only data the public pipeline
//! ever sends (see `TELEMETRY.md`). Events are built through the typed
//! constructors below and checked again on the way out ([`Event::valid`]), so
//! nothing free-form leaves the machine: no log message text, paths, branch,
//! repo or PR names, usernames or hostnames. Attribute values are short tokens
//! (`[A-Za-z0-9_.:-]`, ≤ 64 chars), integers or booleans; the one exception is
//! `code.filepath`, which must be a source path inside this repo (`crates/…rs`)
//! as compiled in, never a path on the user's machine.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Every event name the pipeline sends.
pub const EVENTS: &[&str] = &[ERROR, CRASH, COMMAND, FEATURES, HEARTBEAT, PERF];
pub const ERROR: &str = "am.error";
pub const CRASH: &str = "am.crash";
pub const COMMAND: &str = "am.command";
pub const FEATURES: &str = "am.features";
pub const HEARTBEAT: &str = "am.heartbeat";
pub const PERF: &str = "am.perf";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Attr {
    Bool(bool),
    Int(u64),
    Str(String),
}

/// One spooled event (one JSON line in the spool).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    /// Unix milliseconds.
    pub ts: u64,
    pub event: String,
    /// `am`, `am-tui` or `am-hook`.
    pub binary: String,
    pub version: String,
    #[serde(default)]
    pub attrs: BTreeMap<String, Attr>,
}

impl Event {
    fn new(event: &'static str, binary: &str, ts: u64) -> Event {
        Event { ts, event: event.into(), binary: binary.into(), version: env!("CARGO_PKG_VERSION").into(), attrs: BTreeMap::new() }
    }

    /// A token attribute; dropped (not sent at all) when it isn't one.
    pub fn with_str(mut self, key: &str, value: &str) -> Event {
        if valid_key(key) && valid_value(key, value) {
            self.attrs.insert(key.into(), Attr::Str(value.into()));
        }
        self
    }

    pub fn with_int(mut self, key: &str, value: u64) -> Event {
        if valid_key(key) {
            self.attrs.insert(key.into(), Attr::Int(value));
        }
        self
    }

    pub fn with_bool(mut self, key: &str, value: bool) -> Event {
        if valid_key(key) {
            self.attrs.insert(key.into(), Attr::Bool(value));
        }
        self
    }

    /// A WARN/ERROR log event: where it was logged, never what it said.
    pub fn error(binary: &str, ts: u64, level: &str, module: &str, file: &str, line: u32, kind: Option<&str>) -> Event {
        let e = Event::new(ERROR, binary, ts)
            .with_str("level", level)
            .with_str("module", module)
            .with_str("code.filepath", file)
            .with_int("code.lineno", line.into());
        match kind {
            Some(kind) => e.with_str("error.kind", kind),
            None => e,
        }
    }

    /// A panic: where, not the message.
    pub fn crash(binary: &str, ts: u64, file: &str, line: u32) -> Event {
        Event::new(CRASH, binary, ts).with_str("code.filepath", file).with_int("code.lineno", line.into())
    }

    /// A CLI command, by its clap path (`worktree.create`); `via` is `mcp`
    /// when an MCP tool call ran it, else `cli`.
    pub fn command(ts: u64, path: &str, via: &str) -> Event {
        Event::new(COMMAND, "am", ts).with_str("command", path).with_str("via", via)
    }

    /// How often one TUI command (`Command::id`) ran since the last report.
    pub fn feature(ts: u64, command: &str, count: u64) -> Event {
        Event::new(FEATURES, "am-tui", ts).with_str("command", command).with_int("count", count)
    }

    /// Once a day from the TUI: the setup in broad strokes (attributes from
    /// the caller, each through `with_*`).
    pub fn heartbeat(ts: u64) -> Event {
        Event::new(HEARTBEAT, "am-tui", ts)
    }

    /// One stage's timings over a report period, in microseconds.
    pub fn perf(binary: &str, ts: u64, stage: &str, n: u64, p50_us: u64, p95_us: u64, max_us: u64) -> Event {
        Event::new(PERF, binary, ts)
            .with_str("stage", stage)
            .with_int("n", n)
            .with_int("p50_us", p50_us)
            .with_int("p95_us", p95_us)
            .with_int("max_us", max_us)
    }

    /// The whole event keeps to the schema (re-checked when read back from the
    /// spool, which is just a file).
    pub fn valid(&self) -> bool {
        EVENTS.contains(&self.event.as_str())
            && token(&self.binary)
            && token(&self.version)
            && self.attrs.iter().all(|(k, v)| {
                valid_key(k)
                    && match v {
                        Attr::Str(s) => valid_value(k, s),
                        Attr::Int(_) | Attr::Bool(_) => true,
                    }
            })
    }

    fn severity(&self) -> (u8, &'static str) {
        match (self.event.as_str(), self.attrs.get("level")) {
            (CRASH, _) => (21, "FATAL"),
            (ERROR, Some(Attr::Str(l))) if l == "ERROR" => (17, "ERROR"),
            (ERROR, _) => (13, "WARN"),
            _ => (9, "INFO"),
        }
    }
}

fn valid_key(key: &str) -> bool {
    let mut chars = key.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && key.len() <= 64
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
}

/// A short identifier-like token: no spaces, slashes, `@`, `~`.
pub fn token(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '-'))
}

fn valid_value(key: &str, value: &str) -> bool {
    if key == "code.filepath" {
        return source_path(value);
    }
    token(value)
}

/// `crates/<crate>/…/<file>.rs`: a path in this repo, as `file!()` gives it.
fn source_path(value: &str) -> bool {
    value.len() <= 120
        && value.starts_with("crates/")
        && value.ends_with(".rs")
        && !value.contains("..")
        && value.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-' | '/'))
}

/// One OTLP/JSON logs request for `events` (valid ones only), grouped into a
/// resource per binary + version; `resource` is the per-install part.
pub fn otlp_logs(events: &[Event], resource: &[(&str, String)]) -> Value {
    let mut groups: BTreeMap<(&str, &str), Vec<Value>> = BTreeMap::new();
    for e in events.iter().filter(|e| e.valid()) {
        let (number, text) = e.severity();
        let attributes: Vec<Value> = e.attrs.iter().map(|(k, v)| kv(k, v)).collect();
        groups.entry((&e.binary, &e.version)).or_default().push(json!({
            "timeUnixNano": (u128::from(e.ts) * 1_000_000).to_string(),
            "severityNumber": number,
            "severityText": text,
            "eventName": e.event,
            "body": { "stringValue": e.event },
            "attributes": attributes,
        }));
    }
    let resource_logs: Vec<Value> = groups
        .into_iter()
        .map(|((binary, version), records)| {
            let mut attributes = vec![
                kv("service.name", &Attr::Str("am".into())),
                kv("service.version", &Attr::Str(version.into())),
                kv("am.binary", &Attr::Str(binary.into())),
            ];
            attributes.extend(resource.iter().map(|(k, v)| kv(k, &Attr::Str(v.clone()))));
            json!({
                "resource": { "attributes": attributes },
                "scopeLogs": [{ "scope": { "name": "am" }, "logRecords": records }],
            })
        })
        .collect();
    json!({ "resourceLogs": resource_logs })
}

fn kv(key: &str, value: &Attr) -> Value {
    let value = match value {
        Attr::Str(s) => json!({ "stringValue": s }),
        // OTLP/JSON carries 64-bit integers as strings.
        Attr::Int(n) => json!({ "intValue": n.to_string() }),
        Attr::Bool(b) => json!({ "boolValue": b }),
    };
    json!({ "key": key, "value": value })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_form_values_never_make_it_into_an_event() {
        let leaks = [
            "/Users/someone/work/repo",
            "~/work",
            "someone@example.com",
            "feature/secret-branch",
            "gh failed: error connecting",
            "../../etc/passwd",
            "",
        ];
        for leak in leaks {
            let e = Event::error("am", 1, "WARN", leak, leak, 7, Some(leak));
            assert!(e.valid());
            let body = serde_json::to_string(&otlp_logs(&[e], &[])).unwrap();
            assert!(!body.contains(leak) || leak.is_empty(), "{leak} leaked: {body}");
            assert_eq!(Event::command(1, leak, "cli").attrs.get("command"), None, "{leak}");
        }
    }

    #[test]
    fn source_paths_are_kept_only_when_they_are_repo_paths() {
        let e = Event::error("am-tui", 1, "WARN", "github", "crates/am-core/src/integrations/github.rs", 120, Some("rate_limited"));
        assert_eq!(e.attrs["code.filepath"], Attr::Str("crates/am-core/src/integrations/github.rs".into()));
        assert_eq!(e.attrs["error.kind"], Attr::Str("rate_limited".into()));
        let abs = Event::crash("am", 1, "/Users/me/.cargo/registry/src/x/lib.rs", 3);
        assert_eq!(abs.attrs.get("code.filepath"), None);
    }

    #[test]
    fn a_tampered_spool_line_is_invalid() {
        let mut e = Event::command(1, "worktree.list", "cli");
        assert!(e.valid());
        e.attrs.insert("command".into(), Attr::Str("/home/me".into()));
        assert!(!e.valid());
        let mut e = Event::command(1, "worktree.list", "cli");
        e.event = "am.anything".into();
        assert!(!e.valid());
        // and invalid events are left out of the request
        let body = otlp_logs(&[e], &[]);
        assert_eq!(body["resourceLogs"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn otlp_body_groups_by_binary_and_version_and_types_values() {
        let events = [
            Event::command(1_793_312_213_923, "repo.add", "cli"),
            Event::feature(1_793_312_213_924, "newWorktree", 3),
            Event::error("am-tui", 5, "ERROR", "db", "crates/am-core/src/db.rs", 9, None),
        ];
        let body = otlp_logs(&events, &[("am.install_id", "abc".into())]);
        let groups = body["resourceLogs"].as_array().unwrap();
        assert_eq!(groups.len(), 2);
        let am = &groups[0];
        assert!(am["resource"]["attributes"].as_array().unwrap().iter().any(|a| a["key"] == "am.install_id"));
        let rec = &am["scopeLogs"][0]["logRecords"][0];
        assert_eq!(rec["timeUnixNano"], "1793312213923000000");
        assert_eq!(rec["eventName"], "am.command");
        let tui = &groups[1]["scopeLogs"][0]["logRecords"];
        assert_eq!(tui[0]["attributes"][1], json!({"key": "count", "value": {"intValue": "3"}}));
        assert_eq!(tui[1]["severityText"], "ERROR");
    }

    /// One of every event, as the relay's tests read it
    /// (`telemetry/relay/test/fixtures/client-body.json`): a schema change on
    /// either side fails a test. `AM_UPDATE_FIXTURES=1` rewrites the file.
    #[test]
    fn the_relay_fixture_is_what_the_client_sends() {
        let ts = 1_793_312_213_923;
        let events = [
            Event::command(ts, "worktree.create", "mcp"),
            Event::feature(ts, "newWorktree", 3),
            Event::heartbeat(ts)
                .with_str("mode", "full")
                .with_str("ide", "vscode")
                .with_int("repos", 2)
                .with_bool("github", true)
                .with_int("sessions.claude", 3),
            Event::perf("am-tui", ts, "collector.git", 91, 17_380, 77_110, 735_160),
            Event::error("am-tui", ts, "WARN", "github", "crates/am-core/src/integrations/github.rs", 120, Some("rate_limited")),
            Event::crash("am-hook", ts, "crates/am-hook/src/lib.rs", 88),
        ];
        let mut events = events.to_vec();
        for e in &mut events {
            e.version = "2.2.0".into();
        }
        let resource = [
            ("os.type", "darwin".to_string()),
            ("host.arch", "arm64".to_string()),
            ("am.install_method", "homebrew".to_string()),
            ("am.install_id", "3f0c1f5e-1d2b-4c1e-9a55-0d7a1c2b3e4f".to_string()),
        ];
        let body = serde_json::to_string_pretty(&otlp_logs(&events, &resource)).unwrap() + "\n";
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../telemetry/relay/test/fixtures/client-body.json");
        if std::env::var_os("AM_UPDATE_FIXTURES").is_some() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &body).unwrap();
        }
        let fixture = std::fs::read_to_string(&path).unwrap_or_default();
        assert_eq!(fixture, body, "the relay fixture is stale: AM_UPDATE_FIXTURES=1 cargo test -p am-core relay_fixture");
    }

    #[test]
    fn events_round_trip_through_a_spool_line() {
        let e = Event::perf("am-tui", 9, "collector.cycle", 91, 17_380, 77_110, 735_160).with_bool("slow", false);
        let line = serde_json::to_string(&e).unwrap();
        assert_eq!(serde_json::from_str::<Event>(&line).unwrap(), e);
    }
}
