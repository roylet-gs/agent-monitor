//! Problems a manifest plugin ran into while it was used, aggregated in the
//! `plugin_issues` table (schema v4) and read back by `am harness issues` and
//! the training-mode edit prompt. Each new (plugin, kind, key) logs one WARN
//! under the `plugin:<id>` module; repeats only bump the count. Built-ins
//! never record issues.

use rusqlite::Connection;

use serde_json::Value;

use super::manifest::Manifest;
use crate::db;
use crate::status::{CANONICAL_TOOLS, HookEvent};

/// A native event the manifest doesn't map (capture-only events included,
/// so the agent sees which of them fired).
pub const UNMAPPED_EVENT: &str = "unmapped_event";
/// A tool name with no `tools` mapping that isn't already canonical.
pub const UNMAPPED_TOOL: &str = "unmapped_tool";
/// `session_id` or `cwd` missing after normalisation.
pub const MISSING_FIELD: &str = "missing_field";
/// No `.git` above the session's directory.
pub const NO_WORKTREE: &str = "no_worktree";
/// The payload wasn't JSON.
pub const BAD_PAYLOAD: &str = "bad_payload";
/// Hooks fired for an id with no loadable manifest.
pub const UNKNOWN_PLUGIN: &str = "unknown_plugin";
/// The sessions command failed, timed out or printed something unparseable.
pub const DISCOVERY_FAILED: &str = "discovery_failed";

pub const ALL_KINDS: [&str; 7] =
    [UNMAPPED_EVENT, UNMAPPED_TOOL, MISSING_FIELD, NO_WORKTREE, BAD_PAYLOAD, UNKNOWN_PLUGIN, DISCOVERY_FAILED];

/// The log module of a plugin's issues and draft events.
pub fn log_module(id: &str) -> String {
    format!("plugin:{id}")
}

/// One problem to record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub kind: &'static str,
    pub key: String,
    pub sample: Option<String>,
}

impl Issue {
    pub fn new(kind: &'static str, key: impl Into<String>, sample: Option<String>) -> Issue {
        Issue { kind, key: key.into(), sample: sample.map(|s| crate::status::truncate(&s, 2000)) }
    }
}

/// Record issues for plugin `id`, logging the first occurrence of each.
pub fn record(conn: &Connection, id: &str, issues: &[Issue]) {
    for i in issues {
        match db::record_plugin_issue(conn, id, i.kind, &i.key, i.sample.as_deref()) {
            Ok(true) => crate::warn!(&log_module(id), "New issue {} {}: {}", i.kind, i.key, i.sample.as_deref().unwrap_or("-")),
            Ok(false) => {}
            Err(e) => crate::debug!(&log_module(id), "Couldn't record issue {} {}: {e}", i.kind, i.key),
        }
    }
}

/// Like [`record`], opening the DB only when there is something to write.
pub fn record_now(id: &str, issues: &[Issue]) {
    if issues.is_empty() {
        return;
    }
    if let Ok(conn) = db::open() {
        record(&conn, id, issues);
    }
}

/// What the hook path finds wrong with one event of a manifest plugin.
/// `native` is the native event name (`--event` or `hook_event_name`).
pub fn hook_issues(m: &Manifest, native: Option<&str>, payload: &Value, ev: &HookEvent) -> Vec<Issue> {
    let sample = || Some(payload.to_string());
    let mut out = Vec::new();
    if payload.is_null() {
        out.push(Issue::new(BAD_PAYLOAD, native.unwrap_or("unknown"), None));
    }
    match native {
        Some(n) if !m.maps_event(n) => out.push(Issue::new(UNMAPPED_EVENT, n, sample())),
        None => out.push(Issue::new(UNMAPPED_EVENT, "unknown", sample())),
        Some(_) => {}
    }
    let tool = payload.get(m.fields.get("tool_name").map(String::as_str).unwrap_or("tool_name")).and_then(Value::as_str);
    if let Some(t) = tool
        && !t.is_empty()
        && !m.maps_tool(t)
        && !CANONICAL_TOOLS.contains(&t)
        && !t.starts_with("mcp")
    {
        out.push(Issue::new(UNMAPPED_TOOL, t, ev.tool_input.as_ref().map(Value::to_string)));
    }
    if !payload.is_null() && native.is_some_and(|n| m.maps_event(n)) {
        if ev.session_id.is_none() {
            out.push(Issue::new(MISSING_FIELD, "session_id", sample()));
        }
        if ev.cwd.is_none() {
            out.push(Issue::new(MISSING_FIELD, "cwd", sample()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const M: &str = r#"{
      "id": "mycli", "name": "My CLI", "program": "mycli",
      "hooks": { "settingsPath": "~/.mycli/settings.json", "projectDirVar": "MYCLI_DIR" },
      "events": { "ToolStart": "PreToolUse", "TurnDone": "Stop" },
      "tools": { "shell": "Bash" },
      "fields": { "session_id": "conversation" }
    }"#;

    fn kinds(issues: &[Issue]) -> Vec<(&str, &str)> {
        issues.iter().map(|i| (i.kind, i.key.as_str())).collect()
    }

    #[test]
    fn finds_unmapped_events_tools_and_missing_fields() {
        let m = Manifest::parse(M).unwrap();
        let check = |native: Option<&str>, p: Value| {
            let ev = m.normalize(&p, native);
            hook_issues(&m, native, &p, &ev)
        };
        let ok = json!({"conversation":"c","cwd":"/w","tool_name":"shell"});
        assert!(check(Some("ToolStart"), ok).is_empty());
        let p = json!({"conversation":"c","cwd":"/w","tool_name":"grep_files"});
        assert_eq!(kinds(&check(Some("ToolStart"), p)), [(UNMAPPED_TOOL, "grep_files")]);
        let p = json!({"conversation":"c","cwd":"/w","tool_name":"mcp__x__y"});
        assert!(check(Some("ToolStart"), p).is_empty());
        let p = json!({"conversation":"c","cwd":"/w"});
        assert_eq!(kinds(&check(Some("BeforeModel"), p)), [(UNMAPPED_EVENT, "BeforeModel")]);
        let p = json!({"cwd":"/w"});
        assert_eq!(kinds(&check(Some("TurnDone"), p)), [(MISSING_FIELD, "session_id")]);
        assert_eq!(kinds(&check(None, Value::Null)), [(BAD_PAYLOAD, "unknown"), (UNMAPPED_EVENT, "unknown")]);
    }
}
