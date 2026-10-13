//! Strict, complete validation of a plugin manifest for `am harness validate`
//! and `add`: every problem at once, each with a JSON path and a hint, so an
//! agent can fix them in one pass. [`Manifest::parse`] (the loader) stops at
//! the first error; this reports them all, then adds warnings about things
//! that load fine but probably don't work, checked against captured events
//! when there are any.

use std::collections::BTreeSet;
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use super::capture::Capture;
use super::manifest::{CANONICAL_EVENTS, Manifest, expand_home, valid_event_name, valid_id};
use super::{BUILTIN, HarnessKind};
use crate::status::CANONICAL_TOOLS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Problem {
    /// JSON pointer into the manifest (`/hooks/settingsPath`), `/` for the whole file.
    pub path: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.path, self.message)?;
        if let Some(h) = &self.hint {
            write!(f, " — {h}")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Report {
    pub id: Option<String>,
    pub errors: Vec<Problem>,
    pub warnings: Vec<Problem>,
}

impl Report {
    pub fn ok(&self) -> bool {
        self.errors.is_empty()
    }

    fn error(&mut self, path: &str, message: impl Into<String>, hint: Option<&str>) {
        self.errors.push(Problem { path: path.into(), message: message.into(), hint: hint.map(str::to_owned) });
    }

    fn warn(&mut self, path: &str, message: impl Into<String>, hint: Option<&str>) {
        self.warnings.push(Problem { path: path.into(), message: message.into(), hint: hint.map(str::to_owned) });
    }
}

/// Top-level keys a manifest may have.
pub const KNOWN_KEYS: [&str; 17] = [
    "id", "name", "shortName", "program", "configDir", "hooks", "events", "tools", "fields", "notificationTypes",
    "resume", "sessions", "process", "notes", "status", "training", "$schema",
];
const REQUIRED: [&str; 5] = ["id", "name", "program", "hooks", "events"];

/// What the checks compare against: the other registered harnesses (for id
/// and hook-file clashes) and the plugin's captured events.
pub struct Context<'a> {
    pub others: Vec<HarnessKind>,
    pub captures: &'a [Capture],
}

impl Context<'_> {
    /// Every registered harness, no captures.
    pub fn registered() -> Context<'static> {
        Context { others: super::all().to_vec(), captures: &[] }
    }
}

pub fn validate(text: &str, ctx: &Context) -> Report {
    let mut r = Report::default();
    let v: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            r.error("/", format!("not valid JSON: {e}"), Some("fix the syntax; comments aren't allowed"));
            return r;
        }
    };
    let Some(obj) = v.as_object() else {
        r.error("/", "must be a JSON object", None);
        return r;
    };
    r.id = obj.get("id").and_then(Value::as_str).map(str::to_owned);

    for k in obj.keys().filter(|k| !KNOWN_KEYS.contains(&k.as_str())) {
        r.error(&format!("/{k}"), "unknown key", Some(&format!("allowed: {}", KNOWN_KEYS.join(", "))));
    }
    for k in REQUIRED.iter().filter(|k| !obj.contains_key(**k)) {
        r.error(&format!("/{k}"), "missing", Some("required; see `am harness schema`"));
    }
    structural(&v, &mut r);

    // Type errors and nested unknown keys come from the loader itself.
    let parsed = match Manifest::parse(text) {
        Ok(m) => Some(m),
        Err(e) => {
            if r.errors.is_empty() {
                r.error("/", e, Some("see `am harness schema` for each field's type"));
            }
            None
        }
    };
    if let Some(m) = &parsed {
        clashes(m, ctx, &mut r);
        warnings(m, ctx, &mut r);
    }
    r
}

/// The checks the loader does, but all of them.
fn structural(v: &Value, r: &mut Report) {
    if let Some(id) = v.get("id").and_then(Value::as_str)
        && !valid_id(id)
    {
        r.error("/id", format!("\"{id}\" must be 1-32 chars of a-z, 0-9, -"), Some("e.g. \"cursor-agent\""));
    }
    if let Some(p) = v.get("program").and_then(Value::as_str)
        && (p.is_empty() || p.contains(char::is_whitespace))
    {
        r.error("/program", "must be a single executable name", Some("arguments go in resume / training.promptArgs"));
    }
    if let Some(var) = v.pointer("/hooks/projectDirVar").and_then(Value::as_str)
        && (var.is_empty() || !var.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
    {
        r.error("/hooks/projectDirVar", format!("\"{var}\" must be an env var name"), Some("look at env.dirs in `am harness captures <id> --full`"));
    }
    if v.get("hooks").is_some() && v.pointer("/hooks/settingsPath").is_none() {
        r.error("/hooks/settingsPath", "missing", Some("the file am writes the hook entries into, e.g. \"~/.mycli/settings.json\""));
    }
    if let Some(events) = v.get("events").and_then(Value::as_object) {
        if events.is_empty() {
            r.error("/events", "must map at least one native event", None);
        }
        for (native, canon) in events {
            let path = format!("/events/{native}");
            if !valid_event_name(native) {
                r.error(&path, "event name must be alphanumeric (plus - _ . :)", None);
            }
            match canon.as_str() {
                Some(c) if CANONICAL_EVENTS.contains(&c) => {}
                Some(c) => r.error(&path, format!("unknown canonical event \"{c}\""), Some(&format!("one of {}", CANONICAL_EVENTS.join(", ")))),
                None => r.error(&path, "must be a string", Some("list events you only want recorded in training.captureEvents")),
            }
        }
    }
    if let Some(caps) = v.pointer("/training/captureEvents").and_then(Value::as_array) {
        for (i, e) in caps.iter().enumerate() {
            if !e.as_str().is_some_and(valid_event_name) {
                r.error(&format!("/training/captureEvents/{i}"), "must be an alphanumeric event name", None);
            }
        }
    }
    if let Some(tools) = v.get("tools").and_then(Value::as_object) {
        for (native, canon) in tools {
            if let Some(c) = canon.as_str()
                && !CANONICAL_TOOLS.contains(&c)
            {
                r.warnings.push(Problem {
                    path: format!("/tools/{native}"),
                    message: format!("\"{c}\" isn't a tool the status rules know"),
                    hint: Some(format!("known: {}", CANONICAL_TOOLS.join(", "))),
                });
            }
        }
    }
}

/// Ids and hook files are shared with every other harness.
fn clashes(m: &Manifest, ctx: &Context, r: &mut Report) {
    if BUILTIN.iter().any(|k| k.id() == m.id) {
        r.error("/id", format!("\"{}\" is a built-in harness", m.id), Some("pick another id"));
    }
    let mine = expand_home(&m.hooks.settings_path);
    for k in ctx.others.iter().filter(|k| k.id() != m.id) {
        if k.provider().settings_path() == mine {
            r.error(
                "/hooks/settingsPath",
                format!("{} is already used by {}", m.hooks.settings_path, k.id()),
                Some("two plugins can't share a hook file: installing one would replace the other's entries"),
            );
        }
    }
}

fn on_path(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(program).is_file()))
}

fn warnings(m: &Manifest, ctx: &Context, r: &mut Report) {
    let targets: BTreeSet<&str> = m.events.values().filter_map(Value::as_str).collect();
    if !targets.contains("Stop") {
        r.warn("/events", "nothing maps to Stop", Some("without a turn-end event a session never shows done"));
    }
    if !targets.contains("UserPromptSubmit") {
        r.warn("/events", "nothing maps to UserPromptSubmit", Some("the session only turns executing on its first tool call"));
    }
    if !targets.contains("PreToolUse") {
        r.warn("/events", "nothing maps to PreToolUse", Some("tool activity and AskUserQuestion → waiting won't show"));
    }
    let tool_targets: BTreeSet<&str> = m.tools.values().map(String::as_str).collect();
    for t in ["Bash", "AskUserQuestion"] {
        if !tool_targets.contains(t) && !m.tools.is_empty() {
            r.warn("/tools", format!("no tool maps to {t}"), None);
        }
    }
    if !on_path(&m.program) {
        r.warn("/program", format!("{} isn't on $PATH", m.program), Some("am runs it to start and resume sessions"));
    }
    match &m.config_dir {
        None => r.warn("/configDir", "missing", Some("without it am never installs this plugin's hooks by default")),
        Some(d) if !expand_home(d).is_dir() => r.warn("/configDir", format!("{d} doesn't exist"), None),
        _ => {}
    }
    if m.process.names.is_empty() && m.process.argv_contains.is_empty() {
        r.warn("/process", "empty", Some("the harness and its tools will show as the worktree's running processes"));
    }
    if m.sessions.is_some() {
        r.warn("/sessions/command", "runs a program on every refresh", Some("make sure it is fast and read-only"));
    }
    if ctx.captures.is_empty() {
        return;
    }
    let seen: BTreeSet<&str> = ctx.captures.iter().map(|c| c.native_event.as_str()).collect();
    let capture_only: BTreeSet<&str> = m.capture_events().iter().map(String::as_str).collect();
    for e in seen.iter().filter(|e| !m.maps_event(e) && !capture_only.contains(**e)) {
        r.warn(&format!("/events/{e}"), "captured but not mapped", Some("map it, or list it in training.captureEvents"));
    }
    for e in m.events.keys().filter(|e| !seen.contains(e.as_str())) {
        r.warn(&format!("/events/{e}"), "mapped but never captured", Some("trigger it, or check the name"));
    }
}

/// Validate a manifest file on disk against the registered harnesses.
pub fn validate_file(path: &Path, captures: &[Capture]) -> Report {
    match std::fs::read_to_string(path) {
        Ok(text) => validate(&text, &Context { others: super::all().to_vec(), captures }),
        Err(e) => {
            let mut r = Report::default();
            r.error("/", format!("can't read {}: {e}", path.display()), None);
            r
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn base() -> Value {
        json!({
            "id": "mycli", "name": "My CLI", "program": "sh", "configDir": "/",
            "hooks": { "settingsPath": "~/.mycli/settings.json", "projectDirVar": "MYCLI_DIR" },
            "events": { "Prompt": "UserPromptSubmit", "ToolStart": "PreToolUse", "TurnDone": "Stop" },
            "tools": { "shell": "Bash", "ask": "AskUserQuestion" },
            "process": { "names": ["mycli"] }
        })
    }

    fn run(v: &Value) -> Report {
        validate(&v.to_string(), &Context { others: BUILTIN.to_vec(), captures: &[] })
    }

    #[test]
    fn a_good_manifest_has_no_errors_or_warnings() {
        let r = run(&base());
        assert!(r.ok(), "{:?}", r.errors);
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
        assert_eq!(r.id.as_deref(), Some("mycli"));
    }

    #[test]
    fn reports_every_error_with_a_path() {
        let mut v = base();
        v["id"] = "My CLI".into();
        v["program"] = "my cli".into();
        v["events"]["Bad"] = "Bogus".into();
        v["surprise"] = 1.into();
        v.as_object_mut().unwrap().remove("name");
        let r = run(&v);
        let paths: Vec<&str> = r.errors.iter().map(|p| p.path.as_str()).collect();
        for want in ["/id", "/program", "/events/Bad", "/surprise", "/name"] {
            assert!(paths.contains(&want), "{want} missing from {paths:?}");
        }
        assert!(r.errors.iter().all(|e| !e.message.is_empty()));
    }

    #[test]
    fn refuses_builtin_ids_and_shared_hook_files() {
        let mut v = base();
        v["id"] = "gemini".into();
        assert!(run(&v).errors.iter().any(|e| e.message.contains("built-in")));
        let mut v = base();
        v["hooks"]["settingsPath"] = "~/.claude/settings.json".into();
        let r = run(&v);
        let e = r.errors.iter().find(|e| e.path == "/hooks/settingsPath").expect("clash");
        assert!(e.message.contains("already used by claude"), "{e}");
    }

    #[test]
    fn warns_about_gaps_and_capture_mismatches() {
        let mut v = base();
        v["events"] = json!({ "ToolStart": "PreToolUse" });
        v["process"] = json!({});
        v["program"] = "surely-not-installed-xyz".into();
        let dir = tempfile::tempdir().unwrap();
        let cap = dir.path().join("c.json");
        std::fs::write(&cap, r#"{"hook_event_name":"AfterModel"}"#).unwrap();
        let caps = vec![crate::harness::capture::read(&cap).unwrap()];
        let r = validate(&v.to_string(), &Context { others: BUILTIN.to_vec(), captures: &caps });
        assert!(r.ok());
        let msgs: Vec<String> = r.warnings.iter().map(ToString::to_string).collect();
        let has = |s: &str| msgs.iter().any(|m| m.contains(s));
        assert!(has("nothing maps to Stop"), "{msgs:?}");
        assert!(has("isn't on $PATH"));
        assert!(has("/process: empty"));
        assert!(has("/events/AfterModel: captured but not mapped"));
        assert!(has("/events/ToolStart: mapped but never captured"));
    }

    #[test]
    fn capture_only_events_are_strings_not_mappings() {
        let mut v = base();
        v["events"]["BeforeModel"] = Value::Null;
        assert!(run(&v).errors.iter().any(|e| e.path == "/events/BeforeModel"));
        let mut v = base();
        v["training"] = json!({ "captureEvents": ["BeforeModel", "bad name"] });
        let r = run(&v);
        assert_eq!(r.errors.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(), ["/training/captureEvents/1"]);
    }
}
