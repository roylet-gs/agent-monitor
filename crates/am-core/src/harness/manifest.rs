//! Declarative harness plugins: a JSON manifest in `<app dir>/agents/<id>.json`
//! describes an agent CLI whose hooks follow the common Claude-style shape,
//! so adding one needs no code and no rebuild. Gemini's built-in provider is
//! this same manifest format (`gemini.json`) plus custom session discovery.
//!
//! ```json
//! {
//!   "id": "mycli", "name": "My CLI", "shortName": "MyCLI", "program": "mycli",
//!   "configDir": "~/.mycli",
//!   "hooks": { "settingsPath": "~/.mycli/settings.json", "projectDirVar": "MYCLI_PROJECT_DIR",
//!              "matcher": "*", "timeoutMs": 5000, "name": "agent-monitor" },
//!   "events": { "PromptSubmitted": "UserPromptSubmit", "TurnDone": "Stop",
//!               "ToolStart": "PreToolUse", "ToolEnd": "PostToolUse" },
//!   "tools": { "shell": "Bash", "ask": "AskUserQuestion" },
//!   "fields": { "last_assistant_message": "response" },
//!   "notificationTypes": { "approval": "permission_prompt" },
//!   "resume": { "session": ["--resume", "{id}"], "continue": ["--continue"] },
//!   "sessions": { "command": ["mycli", "sessions", "--json", "{root}"] },
//!   "process": { "names": ["mycli"], "argvContains": ["/@acme/mycli/"] },
//!   "notes": "hooks only run in trusted folders",
//!   "mcp": { "file": "~/.mycli/settings.json", "key": "mcpServers", "entry": {"type": "stdio"} }
//! }
//! ```
//!
//! - `events` maps native → canonical event names (Claude Code's vocabulary:
//!   `SessionStart`, `SessionEnd`, `UserPromptSubmit`, `PreToolUse`,
//!   `PostToolUse`, `Stop`, `StopFailure`, `Notification`, `PermissionRequest`,
//!   `SubagentStart`, `SubagentStop`). Hooks are installed for exactly these keys.
//! - `fields` renames canonical payload keys (`session_id`, `transcript_path`,
//!   `cwd`, `tool_name`, `tool_input`, `last_assistant_message`, `message`,
//!   `notification_type`, `stop_hook_active`, `permission_mode`, `agent_id`,
//!   `agent_type`, …) to the agent's own; unlisted keys are read as-is.
//! - `fields` values starting with `/` are JSON pointers into the payload
//!   (`"cwd": "/workspace_roots/0"`).
//! - `hooks` shapes the entries am writes. Defaults give Claude's nested
//!   `{"matcher", "hooks": [{"type": "command", "command", "timeout"}]}` under
//!   a top-level `hooks` key. `layout: "flat"` writes one hook object per
//!   entry (Cursor, Copilot); `commandKey` / `timeoutKey` rename those keys,
//!   `timeoutUnit` is `ms` (default) or `s`, `entry` adds keys to every hook
//!   object, `file` adds top-level keys when missing (`{"version": 1}`), and
//!   `hooksKey: ""` puts events at the top level of the file. Without
//!   `projectDirVar` the command passes `--worktree ""` and am-hook uses the
//!   payload's `cwd`.
//! - `sessions.command` (optional) prints a JSON array of
//!   `{"id", "cwd", "modified"}` (modified = unix seconds) for `{root}`;
//!   without it, resuming falls back to `resume.continue`. Built-ins without
//!   their own discovery offer the sessions am's hooks recorded instead.
//! - `mcp` (optional) is where the CLI reads MCP servers: a JSON `file` with a
//!   `name → {command, args}` map under `key` (default `mcpServers`), plus
//!   `entry` keys for am's server. `am mcp install` registers `am mcp` there.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::{Harness, HarnessKind, Origin, ProcessSpec};
use crate::launch::Session;
use crate::paths;
use crate::status::HookEvent;

/// Canonical event names a manifest may map to.
pub const CANONICAL_EVENTS: [&str; 11] = [
    "SessionStart",
    "SessionEnd",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "Stop",
    "StopFailure",
    "Notification",
    "PermissionRequest",
    "SubagentStart",
    "SubagentStop",
];

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub short_name: Option<String>,
    pub program: String,
    /// Presence check: the CLI has been used here when this dir exists.
    #[serde(default)]
    pub config_dir: Option<String>,
    pub hooks: HookSpec,
    /// Native → canonical, in install order.
    pub events: Map<String, Value>,
    #[serde(default)]
    pub tools: HashMap<String, String>,
    #[serde(default)]
    pub fields: HashMap<String, String>,
    #[serde(default)]
    pub notification_types: HashMap<String, String>,
    #[serde(default)]
    pub resume: ResumeSpec,
    #[serde(default)]
    pub sessions: Option<SessionsSpec>,
    #[serde(default)]
    pub process: ProcessSpec,
    /// A caveat shown next to the hook status, e.g. a trust requirement.
    #[serde(default)]
    pub notes: Option<String>,
    /// Where the CLI reads MCP servers, for `am mcp install`.
    #[serde(default)]
    pub mcp: Option<McpSpec>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpSpec {
    /// JSON file holding the server map (`~/.cursor/mcp.json`).
    pub file: String,
    /// Key of the `name → server` map.
    #[serde(default = "default_mcp_key")]
    pub key: String,
    /// Extra keys for am's server entry (`{"type": "stdio"}`).
    #[serde(default)]
    pub entry: Map<String, Value>,
}

fn default_mcp_key() -> String {
    "mcpServers".into()
}

impl McpSpec {
    pub fn config(&self) -> super::McpConfig {
        super::McpConfig { path: expand_home(&self.file), key: self.key.clone(), entry: self.entry.clone() }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HookSpec {
    pub settings_path: String,
    /// Env var the CLI sets to the session's project dir (`$VAR` in the
    /// command). Without one, `--worktree ""` makes am-hook use the payload's
    /// `cwd`.
    #[serde(default)]
    pub project_dir_var: Option<String>,
    #[serde(default)]
    pub matcher: Option<String>,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
    /// The unit the CLI reads the timeout in.
    #[serde(default)]
    pub timeout_unit: TimeoutUnit,
    /// Key of the timeout in a hook object (`timeout`, Copilot's `timeoutSec`).
    #[serde(default = "default_timeout_key")]
    pub timeout_key: String,
    /// Key of the command in a hook object (`command`, Copilot's `bash`).
    #[serde(default = "default_command_key")]
    pub command_key: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub layout: HookLayout,
    /// Extra keys for each hook object, e.g. `{"type": "command"}`.
    #[serde(default)]
    pub entry: Map<String, Value>,
    /// Top-level keys the hook file needs, added when missing (`{"version": 1}`).
    #[serde(default)]
    pub file: Map<String, Value>,
    /// The key holding `event → entries`; `""` when the file is that map.
    #[serde(default = "default_hooks_key")]
    pub hooks_key: String,
}

fn default_hooks_key() -> String {
    "hooks".into()
}

/// How one entry of `hooks.<event>` is shaped.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HookLayout {
    /// Claude's `{"matcher": …, "hooks": [{"type": "command", "command": …}]}`.
    #[default]
    Nested,
    /// One hook object per entry: `{"command": …, "timeout": …}` (Cursor, Copilot).
    Flat,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TimeoutUnit {
    #[default]
    Ms,
    S,
}

fn default_timeout() -> u64 {
    5_000
}

fn default_timeout_key() -> String {
    "timeout".into()
}

fn default_command_key() -> String {
    "command".into()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResumeSpec {
    /// `{id}` is replaced by the session id.
    #[serde(default = "default_resume_session")]
    pub session: Vec<String>,
    #[serde(default, rename = "continue")]
    pub continue_latest: Vec<String>,
}

impl Default for ResumeSpec {
    fn default() -> Self {
        ResumeSpec { session: default_resume_session(), continue_latest: Vec::new() }
    }
}

fn default_resume_session() -> Vec<String> {
    vec!["--resume".into(), "{id}".into()]
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionsSpec {
    /// argv; `{root}` is replaced by the worktree path.
    pub command: Vec<String>,
}

/// `~/x` → `$HOME/x`.
pub fn expand_home(p: &str) -> PathBuf {
    match p.strip_prefix("~/") {
        Some(rest) => paths::home_dir().join(rest),
        None if p == "~" => paths::home_dir(),
        None => PathBuf::from(p),
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 32 && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn valid_var(v: &str) -> bool {
    !v.is_empty() && v.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

impl Manifest {
    /// A compiled-in manifest; a broken one is a bug caught by the tests.
    pub fn builtin(text: &str) -> Manifest {
        Manifest::parse(text).unwrap_or_else(|e| panic!("built-in harness manifest is invalid: {e}"))
    }

    /// Parse and validate. Errors read as one line for the plugin list.
    pub fn parse(text: &str) -> Result<Manifest, String> {
        let m: Manifest = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if !valid_id(&m.id) {
            return Err(format!("id \"{}\" must be 1-32 chars of a-z, 0-9, -", m.id));
        }
        if let Some(var) = m.hooks.project_dir_var.as_deref().filter(|v| !valid_var(v)) {
            return Err(format!("hooks.projectDirVar \"{var}\" must be an env var name"));
        }
        if m.program.is_empty() || m.program.contains(char::is_whitespace) {
            return Err("program must be a single executable name".into());
        }
        if m.events.is_empty() {
            return Err("events must map at least one native event".into());
        }
        for (native, canon) in &m.events {
            let Some(canon) = canon.as_str() else { return Err(format!("events.{native} must be a string")) };
            if !CANONICAL_EVENTS.contains(&canon) {
                return Err(format!("events.{native}: unknown canonical event \"{canon}\""));
            }
            if native.is_empty() || !native.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') {
                return Err(format!("event name \"{native}\" must be alphanumeric"));
            }
        }
        Ok(m)
    }

    fn field<'a>(&'a self, canonical: &'a str) -> &'a str {
        self.fields.get(canonical).map(String::as_str).unwrap_or(canonical)
    }

    /// The payload value for a canonical key: a top-level key, or a JSON
    /// pointer when the mapping starts with `/` (`/workspace_roots/0`).
    fn lookup<'v>(&self, v: &'v Value, canonical: &str) -> Option<&'v Value> {
        let f = self.field(canonical);
        if f.starts_with('/') { v.pointer(f) } else { v.get(f) }
    }

    fn canonical_event<'a>(&'a self, native: &'a str) -> &'a str {
        self.events.get(native).and_then(Value::as_str).unwrap_or(native)
    }

    /// Native payload → canonical event, driven entirely by the mappings.
    pub fn normalize(&self, v: &Value, event_override: Option<&str>) -> HookEvent {
        let s = |canon: &str| self.lookup(v, canon).and_then(Value::as_str).filter(|x| !x.is_empty()).map(str::to_owned);
        let b = |canon: &str| self.lookup(v, canon).and_then(Value::as_bool).unwrap_or(false);
        let native = event_override.map(str::to_owned).or_else(|| s("hook_event_name")).unwrap_or_else(|| "unknown".into());
        HookEvent {
            event: self.canonical_event(&native).to_owned(),
            session_id: s("session_id"),
            transcript_path: s("transcript_path"),
            cwd: s("cwd"),
            permission_mode: s("permission_mode"),
            tool_name: s("tool_name").map(|t| self.tools.get(&t).cloned().unwrap_or(t)),
            tool_input: self.lookup(v, "tool_input").cloned(),
            stop_hook_active: b("stop_hook_active"),
            last_assistant_message: s("last_assistant_message").filter(|m| !m.trim().is_empty()),
            transcript_summary: s("transcript_summary"),
            message: s("message"),
            notification_type: s("notification_type").map(|t| self.notification_types.get(&t).cloned().unwrap_or(t)),
            permission_prompt: b("permission_prompt"),
            agent_id: s("agent_id"),
            agent_type: self.lookup(v, "agent_type").and_then(Value::as_str).map(str::to_owned),
            agent_transcript_path: s("agent_transcript_path"),
        }
    }

    pub fn hook_events(&self) -> Vec<String> {
        self.events.keys().cloned().collect()
    }

    pub fn hook_entry(&self, event: &str, invocation: &str) -> Value {
        let h = &self.hooks;
        let worktree = h.project_dir_var.as_deref().map(|v| format!("\"${v}\"")).unwrap_or_else(|| "\"\"".into());
        let command = format!("{invocation} --agent {} --worktree {worktree} --event {event}", self.id);
        let timeout = match h.timeout_unit {
            TimeoutUnit::Ms => h.timeout_ms,
            TimeoutUnit::S => h.timeout_ms.div_ceil(1000),
        };
        let mut hook = Map::new();
        if h.layout == HookLayout::Nested {
            hook.insert("type".into(), "command".into());
        }
        hook.insert(h.command_key.clone(), command.into());
        hook.insert(h.timeout_key.clone(), timeout.into());
        if let Some(name) = &h.name {
            hook.insert("name".into(), name.clone().into());
        }
        hook.extend(h.entry.clone());
        let mut entry = match h.layout {
            HookLayout::Nested => json!({ "hooks": [hook] }),
            HookLayout::Flat => Value::Object(hook),
        };
        if let Some(m) = &h.matcher {
            entry["matcher"] = m.clone().into();
        }
        entry
    }

    pub fn resume_args(&self, session_id: Option<&str>, continue_latest: bool) -> Vec<String> {
        match (session_id, continue_latest) {
            (Some(id), _) => self.resume.session.iter().map(|a| a.replace("{id}", id)).collect(),
            (None, true) => self.resume.continue_latest.clone(),
            (None, false) => Vec::new(),
        }
    }

    /// Run `sessions.command` (bounded to 3 s) and parse its JSON array.
    pub fn command_sessions(&self, kind: HarnessKind, root: &Path) -> Vec<Session> {
        let Some(spec) = &self.sessions else { return Vec::new() };
        let root_s = root.to_string_lossy();
        let argv: Vec<String> = spec.command.iter().map(|a| a.replace("{root}", &root_s)).collect();
        let Some((prog, args)) = argv.split_first() else { return Vec::new() };
        let Some(out) = run_bounded(prog, args, Duration::from_secs(3)) else {
            crate::debug!("agents", "{}: sessions command failed or timed out", self.id);
            return Vec::new();
        };
        let Ok(Value::Array(items)) = serde_json::from_slice::<Value>(&out) else { return Vec::new() };
        let mut sessions: Vec<Session> = items
            .iter()
            .filter_map(|it| {
                let id = it.get("id")?.as_str()?.to_owned();
                let cwd = it.get("cwd").and_then(Value::as_str).unwrap_or(&root_s).to_owned();
                if !super::is_within(&cwd, &root_s) {
                    return None;
                }
                let secs = it.get("modified").and_then(Value::as_u64).unwrap_or(0);
                Some(Session { id, cwd, modified: SystemTime::UNIX_EPOCH + Duration::from_secs(secs), agent: kind })
            })
            .collect();
        sessions.sort_by_key(|s| std::cmp::Reverse(s.modified));
        sessions
    }
}

fn run_bounded(prog: &str, args: &[String], limit: Duration) -> Option<Vec<u8>> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    let mut child = Command::new(prog).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });
    let started = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success().then(|| reader.join().ok()).flatten(),
            Ok(None) if started.elapsed() < limit => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

/// A provider made from a manifest alone.
pub struct ManifestProvider {
    pub manifest: Manifest,
    pub source: PathBuf,
}

impl Harness for ManifestProvider {
    fn id(&self) -> &str {
        &self.manifest.id
    }
    fn name(&self) -> &str {
        &self.manifest.name
    }
    fn short_name(&self) -> &str {
        self.manifest.short_name.as_deref().unwrap_or(&self.manifest.name)
    }
    fn program(&self) -> &str {
        &self.manifest.program
    }
    fn origin(&self) -> Origin {
        Origin::Plugin(self.source.clone())
    }
    fn notes(&self) -> Option<&str> {
        self.manifest.notes.as_deref()
    }
    fn is_present(&self) -> bool {
        self.manifest.config_dir.as_deref().is_some_and(|d| expand_home(d).is_dir())
    }
    fn settings_path(&self) -> PathBuf {
        expand_home(&self.manifest.hooks.settings_path)
    }
    fn hook_events(&self) -> Vec<String> {
        self.manifest.hook_events()
    }
    fn hook_entry(&self, event: &str, invocation: &str, _native: bool) -> Value {
        self.manifest.hook_entry(event, invocation)
    }
    fn hook_file_defaults(&self) -> Map<String, Value> {
        self.manifest.hooks.file.clone()
    }
    fn hooks_key(&self) -> &str {
        &self.manifest.hooks.hooks_key
    }
    fn normalize(&self, payload: &Value, event_override: Option<&str>) -> HookEvent {
        self.manifest.normalize(payload, event_override)
    }
    fn discover_sessions(&self, root: &Path) -> Vec<Session> {
        match HarnessKind::get(self.id()) {
            Some(kind) => self.manifest.command_sessions(kind, root),
            None => Vec::new(),
        }
    }
    fn resume_args(&self, session_id: Option<&str>, continue_latest: bool) -> Vec<String> {
        self.manifest.resume_args(session_id, continue_latest)
    }
    fn matches_process(&self, name: &str, argv: &[String]) -> bool {
        self.manifest.process.matches(name, argv)
    }
    fn mcp_config(&self) -> Option<super::McpConfig> {
        self.manifest.mcp.as_ref().map(McpSpec::config)
    }
}

/// Load every `*.json` manifest in `dir`. Bad files and ids that clash with a
/// built-in or an earlier file are reported, not fatal.
pub fn load_dir(dir: &Path, taken: &[&str]) -> (Vec<ManifestProvider>, Vec<String>) {
    let mut out: Vec<ManifestProvider> = Vec::new();
    let mut errors = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return (out, errors) };
    let mut files: Vec<PathBuf> =
        entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json")).collect();
    files.sort();
    for path in files {
        let file = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let parsed = std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| Manifest::parse(&t));
        match parsed {
            Ok(m) if taken.contains(&m.id.as_str()) || out.iter().any(|p| p.manifest.id == m.id) => {
                errors.push(format!("{file}: id \"{}\" is already taken", m.id));
            }
            Ok(manifest) => out.push(ManifestProvider { manifest, source: path }),
            Err(e) => errors.push(format!("{file}: {e}")),
        }
    }
    for e in &errors {
        crate::warn!("agents", "Skipping harness plugin {e}");
    }
    (out, errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = r#"{
      "id": "mycli", "name": "My CLI", "program": "mycli", "configDir": "~/.mycli",
      "hooks": { "settingsPath": "~/.mycli/settings.json", "projectDirVar": "MYCLI_PROJECT_DIR", "matcher": "*" },
      "events": { "PromptSubmitted": "UserPromptSubmit", "ToolStart": "PreToolUse", "TurnDone": "Stop", "Approval": "Notification" },
      "tools": { "shell": "Bash", "ask": "AskUserQuestion" },
      "fields": { "last_assistant_message": "response", "session_id": "conversation" },
      "notificationTypes": { "approval": "permission_prompt" },
      "resume": { "session": ["resume", "{id}"], "continue": ["resume", "--last"] },
      "process": { "names": ["mycli"] }
    }"#;

    #[test]
    fn a_manifest_drives_normalisation_and_the_status_rules() {
        use crate::status::{Status::*, map_event_to_status};
        let m = Manifest::parse(EXAMPLE).unwrap();
        let ev = |p: Value| m.normalize(&p, None);
        let e = ev(json!({"hook_event_name":"ToolStart","tool_name":"shell","conversation":"c1"}));
        assert_eq!((e.event.as_str(), e.tool_name.as_deref(), e.session_id.as_deref()), ("PreToolUse", Some("Bash"), Some("c1")));
        let e = ev(json!({"hook_event_name":"TurnDone","response":"ok"}));
        assert_eq!((e.event.as_str(), e.last_assistant_message.as_deref()), ("Stop", Some("ok")));
        let step = |cur, p| map_event_to_status(&ev(p), cur, 0);
        assert_eq!(step(None, json!({"hook_event_name":"PromptSubmitted"})), Some(Executing));
        assert_eq!(step(Some(Executing), json!({"hook_event_name":"Approval","notification_type":"approval"})), Some(Waiting));
        assert_eq!(step(Some(Executing), json!({"hook_event_name":"ToolStart","tool_name":"ask"})), Some(Waiting));
        assert_eq!(step(Some(Executing), json!({"hook_event_name":"TurnDone"})), Some(Done));
    }

    #[test]
    fn fields_can_be_json_pointers() {
        let m = Manifest::parse(
            r#"{"id":"ptr","name":"Ptr","program":"ptr","hooks":{"settingsPath":"~/.ptr/hooks.json"},
                "events":{"stop":"Stop"},
                "fields":{"session_id":"/conversation_id","cwd":"/workspace_roots/0","hook_event_name":"hook_event_name"}}"#,
        )
        .unwrap();
        let e = m.normalize(&json!({"hook_event_name":"stop","conversation_id":"c-9","workspace_roots":["/w/repo","/w/other"]}), None);
        assert_eq!((e.event.as_str(), e.session_id.as_deref(), e.cwd.as_deref()), ("Stop", Some("c-9"), Some("/w/repo")));
        assert_eq!(m.normalize(&json!({"hook_event_name":"stop","workspace_roots":[]}), None).cwd, None);
    }

    #[test]
    fn hook_entries_resume_and_processes() {
        let m = Manifest::parse(EXAMPLE).unwrap();
        assert_eq!(m.hook_events(), ["PromptSubmitted", "ToolStart", "TurnDone", "Approval"]);
        let e = m.hook_entry("ToolStart", "\"/b/am-hook\" hook-event");
        assert_eq!(e["matcher"], "*");
        assert_eq!(e["hooks"][0]["timeout"], 5_000);
        let cmd = e["hooks"][0]["command"].as_str().unwrap();
        assert_eq!(cmd, "\"/b/am-hook\" hook-event --agent mycli --worktree \"$MYCLI_PROJECT_DIR\" --event ToolStart");
        assert!(crate::hooks_installer::is_am_hook_command(cmd));
        assert_eq!(m.resume_args(Some("x1"), true), ["resume", "x1"]);
        assert_eq!(m.resume_args(None, true), ["resume", "--last"]);
        assert!(m.resume_args(None, false).is_empty());
        assert!(m.process.matches("mycli", &[]));
        assert!(m.process.matches("node", &["node".into(), "/x/bin/mycli".into()]));
        assert!(!m.process.matches("node", &["node".into(), "server.js".into()]));
    }

    #[test]
    fn invalid_manifests_are_rejected_with_a_reason() {
        let bad = |patch: &str, needle: &str| {
            let mut v: Value = serde_json::from_str(EXAMPLE).unwrap();
            let p: Value = serde_json::from_str(patch).unwrap();
            for (k, x) in p.as_object().unwrap() {
                v[k] = x.clone();
            }
            let err = Manifest::parse(&v.to_string()).unwrap_err();
            assert!(err.contains(needle), "{err}");
        };
        bad(r#"{"id":"My CLI"}"#, "id \"My CLI\"");
        bad(r#"{"events":{"X":"Bogus"}}"#, "unknown canonical event");
        bad(r#"{"events":{}}"#, "at least one");
        bad(r#"{"program":"my cli"}"#, "single executable");
        bad(r#"{"surprise":1}"#, "unknown field");
    }

    #[test]
    fn loads_a_directory_and_reports_clashes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.json"), EXAMPLE).unwrap();
        std::fs::write(dir.path().join("b.json"), EXAMPLE).unwrap();
        std::fs::write(dir.path().join("c.json"), EXAMPLE.replace("\"mycli\", \"name\"", "\"claude\", \"name\"")).unwrap();
        std::fs::write(dir.path().join("d.json"), "{ nope").unwrap();
        std::fs::write(dir.path().join("notes.txt"), "ignored").unwrap();
        let (ok, errors) = load_dir(dir.path(), &["claude", "gemini"]);
        assert_eq!(ok.len(), 1);
        assert_eq!(ok[0].manifest.id, "mycli");
        assert_eq!(errors.len(), 3, "{errors:?}");
        assert!(errors[0].starts_with("b.json: id \"mycli\" is already taken"));
        assert!(errors[1].starts_with("c.json: id \"claude\" is already taken"));
        assert!(errors[2].starts_with("d.json:"));
    }

    #[test]
    fn sessions_come_from_the_plugin_command() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("sessions.sh");
        std::fs::write(
            &script,
            "#!/bin/sh\necho '[{\"id\":\"old\",\"cwd\":\"'$1'\",\"modified\":100},{\"id\":\"new\",\"cwd\":\"'$1'/pkg\",\"modified\":200},{\"id\":\"x\",\"cwd\":\"/elsewhere\"}]'\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut v: Value = serde_json::from_str(EXAMPLE).unwrap();
        v["sessions"] = json!({ "command": [script.to_string_lossy(), "{root}"] });
        let m = Manifest::parse(&v.to_string()).unwrap();
        let s = m.command_sessions(HarnessKind::CLAUDE, Path::new("/w/repo"));
        let ids: Vec<&str> = s.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["new", "old"]);
        assert_eq!(s[0].cwd, "/w/repo/pkg");
    }
}
