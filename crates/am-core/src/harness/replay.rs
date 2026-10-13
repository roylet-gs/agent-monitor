//! Dry-run captured hook events through a manifest (`am harness test`): what
//! each native event normalises to, and the status each session would go
//! through. Pure: nothing is written, so a training agent can iterate on a
//! candidate manifest without re-adding it.

use std::collections::{BTreeMap, HashMap};

use serde::Serialize;

use super::capture::Capture;
use super::issues::{self, hook_issues};
use super::manifest::Manifest;
use crate::status::{Status, map_event_to_status};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    pub file: String,
    pub native: String,
    /// Canonical event, `None` when the manifest doesn't map it (no status change).
    pub canonical: Option<String>,
    pub tool: Option<String>,
    pub session_id: Option<String>,
    pub cwd: Option<String>,
    pub from: Option<&'static str>,
    pub to: Option<&'static str>,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Coverage {
    /// Native event → times seen.
    pub seen: BTreeMap<String, usize>,
    /// Seen but not mapped (capture-only events included).
    pub unmapped_events: Vec<String>,
    /// Mapped but never seen.
    pub never_seen: Vec<String>,
    pub unmapped_tools: Vec<String>,
    /// `session_id` / `cwd` → events that lacked it.
    pub missing_fields: BTreeMap<String, usize>,
    pub sessions: usize,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Replay {
    pub steps: Vec<Step>,
    pub coverage: Coverage,
}

impl Replay {
    /// Mapped events that produced a canonical event the rules understand,
    /// and at least one session that ran a turn: the minimum for a plugin to work.
    pub fn has_failures(&self) -> bool {
        !self.coverage.unmapped_events.is_empty() && self.steps.iter().all(|s| s.canonical.is_none())
    }
}

pub fn replay(m: &Manifest, captures: &[Capture]) -> Replay {
    let mut out = Replay::default();
    let mut status: HashMap<String, Option<Status>> = HashMap::new();
    let mut subagents: HashMap<String, u32> = HashMap::new();
    for c in captures {
        let payload = c.payload();
        let native = Some(c.native_event.as_str()).filter(|n| !n.is_empty() && *n != "unknown").map(str::to_owned).or_else(|| m.native_event(payload, None));
        let ev = m.normalize(payload, native.as_deref());
        let found = hook_issues(m, native.as_deref(), payload, &ev);
        let native = native.unwrap_or_else(|| "unknown".into());
        *out.coverage.seen.entry(native.clone()).or_default() += 1;
        for i in &found {
            match i.kind {
                issues::UNMAPPED_EVENT if !out.coverage.unmapped_events.contains(&i.key) => out.coverage.unmapped_events.push(i.key.clone()),
                issues::UNMAPPED_TOOL if !out.coverage.unmapped_tools.contains(&i.key) => out.coverage.unmapped_tools.push(i.key.clone()),
                issues::MISSING_FIELD => *out.coverage.missing_fields.entry(i.key.clone()).or_default() += 1,
                _ => {}
            }
        }
        let mapped = m.maps_event(&native);
        let key = ev.session_id.clone().unwrap_or_else(|| "(no session)".into());
        let (from, to) = if mapped {
            let running = subagents.entry(key.clone()).or_default();
            match ev.event.as_str() {
                "SubagentStart" => *running += 1,
                "SubagentStop" => *running = running.saturating_sub(1),
                _ => {}
            }
            let cur = status.get(&key).copied().flatten();
            let next = map_event_to_status(&ev, cur, *running);
            if let Some(n) = next {
                status.insert(key, Some(n));
            }
            (cur.map(Status::as_str), next.map(Status::as_str))
        } else {
            (None, None)
        };
        out.steps.push(Step {
            file: c.path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default(),
            native,
            canonical: mapped.then(|| ev.event.clone()),
            tool: ev.tool_name.clone(),
            session_id: ev.session_id.clone(),
            cwd: ev.cwd.clone(),
            from,
            to,
            issues: found.iter().map(|i| format!("{} {}", i.kind, i.key)).collect(),
        });
    }
    out.coverage.sessions = status.len();
    out.coverage.never_seen = m.events.keys().filter(|e| !out.coverage.seen.contains_key(*e)).cloned().collect();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::capture;

    #[test]
    fn replays_a_turn_and_reports_coverage() {
        let m = Manifest::parse(
            r#"{ "id": "mycli", "name": "My CLI", "program": "mycli",
                 "hooks": { "settingsPath": "~/.mycli/s.json", "projectDirVar": "D" },
                 "events": { "Prompt": "UserPromptSubmit", "ToolStart": "PreToolUse", "TurnDone": "Stop", "Exit": "SessionEnd" },
                 "tools": { "shell": "Bash" },
                 "status": "draft", "training": { "captureEvents": ["BeforeModel"] } }"#,
        )
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let files = [
            r#"{"hook_event_name":"Prompt","session_id":"s1","cwd":"/w"}"#,
            r#"{"hook_event_name":"BeforeModel","session_id":"s1","cwd":"/w"}"#,
            r#"{"hook_event_name":"ToolStart","session_id":"s1","cwd":"/w","tool_name":"grep_files"}"#,
            r#"{"hook_event_name":"TurnDone","session_id":"s1"}"#,
        ];
        let caps: Vec<_> = files
            .iter()
            .enumerate()
            .map(|(i, body)| {
                let p = dir.path().join(format!("{i:013}-1.0-x.json"));
                std::fs::write(&p, body).unwrap();
                capture::read(&p).unwrap()
            })
            .collect();
        let r = replay(&m, &caps);
        let flow: Vec<(Option<&str>, Option<&str>)> = r.steps.iter().map(|s| (s.canonical.as_deref(), s.to)).collect();
        assert_eq!(
            flow,
            [
                (Some("UserPromptSubmit"), Some("executing")),
                (None, None),
                (Some("PreToolUse"), Some("executing")),
                (Some("Stop"), Some("done")),
            ]
        );
        assert_eq!(r.coverage.unmapped_events, ["BeforeModel"]);
        assert_eq!(r.coverage.unmapped_tools, ["grep_files"]);
        assert_eq!(r.coverage.missing_fields.get("cwd"), Some(&1));
        assert_eq!(r.coverage.never_seen, ["Exit"]);
        assert_eq!(r.coverage.sessions, 1);
        assert!(!r.has_failures());
    }
}
