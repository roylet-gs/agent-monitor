//! A built-in harness written as a manifest: the same JSON format users drop
//! into `<app dir>/agents/`, compiled in with `include_str!`, plus optional
//! Rust session discovery for CLIs whose session store needs code. Without
//! it, the sessions am's own hooks recorded are the ones offered for resume.
//! Gemini,
//! Codex, Qwen Code, Factory Droid, Copilot CLI, Cursor, Junie and Auggie are
//! all one of these.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::{Duration, SystemTime};

use serde_json::{Map, Value};

use super::{Harness, HarnessKind, is_within};
use super::manifest::{Manifest, expand_home};
use crate::launch::Session;
use crate::status::HookEvent;

pub struct Embedded {
    manifest: LazyLock<Manifest>,
    discover: Option<fn(&Path) -> Vec<Session>>,
    /// Picks the hook file and its hooks key at runtime, for a CLI that reads
    /// one of several files (Factory Droid).
    hook_file: Option<fn() -> (PathBuf, &'static str)>,
}

impl Embedded {
    /// `load` parses the embedded JSON (`|| Manifest::builtin(include_str!(…))`).
    pub const fn new(load: fn() -> Manifest, discover: Option<fn(&Path) -> Vec<Session>>) -> Embedded {
        Embedded { manifest: LazyLock::new(load), discover, hook_file: None }
    }

    pub const fn with_hook_file(mut self, f: fn() -> (PathBuf, &'static str)) -> Embedded {
        self.hook_file = Some(f);
        self
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }
}

impl Harness for Embedded {
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

    fn notes(&self) -> Option<&str> {
        self.manifest.notes.as_deref()
    }

    fn is_present(&self) -> bool {
        self.manifest.config_dir.as_deref().is_some_and(|d| expand_home(d).is_dir())
    }

    fn settings_path(&self) -> PathBuf {
        match self.hook_file {
            Some(f) => f().0,
            None => expand_home(&self.manifest.hooks.settings_path),
        }
    }

    fn hook_events(&self) -> Vec<String> {
        self.manifest.hook_events()
    }

    fn hook_file_defaults(&self) -> Map<String, Value> {
        self.manifest.hooks.file.clone()
    }

    fn hooks_key(&self) -> &str {
        match self.hook_file {
            Some(f) => f().1,
            None => &self.manifest.hooks.hooks_key,
        }
    }

    fn hook_entry(&self, event: &str, invocation: &str, _native: bool) -> Value {
        self.manifest.hook_entry(event, invocation)
    }

    fn normalize(&self, payload: &Value, event_override: Option<&str>) -> HookEvent {
        self.manifest.normalize(payload, event_override)
    }

    fn discover_sessions(&self, root: &Path) -> Vec<Session> {
        match self.discover {
            Some(f) => f(root),
            None => HarnessKind::get(self.id()).map(|k| recorded_sessions(k, root)).unwrap_or_default(),
        }
    }

    fn resume_args(&self, session_id: Option<&str>, continue_latest: bool) -> Vec<String> {
        self.manifest.resume_args(session_id, continue_latest)
    }

    fn matches_process(&self, name: &str, argv: &[String]) -> bool {
        self.manifest.process.matches(name, argv)
    }

    fn mcp_config(&self) -> Option<super::McpConfig> {
        self.manifest.mcp.as_ref().map(super::manifest::McpSpec::config)
    }
}

/// Sessions of `kind` the hooks saw at `root` or below, newest first.
pub fn recorded_sessions(kind: HarnessKind, root: &Path) -> Vec<Session> {
    let root = root.to_string_lossy();
    let rows = match crate::db::open().and_then(|c| crate::db::recorded_sessions(&c, kind)) {
        Ok(rows) => rows,
        Err(e) => {
            crate::debug!("agents", "{}: recorded sessions unavailable: {e}", kind.id());
            return Vec::new();
        }
    };
    rows.into_iter()
        .filter(|(_, dir, _)| is_within(dir, &root))
        .map(|(id, cwd, updated)| {
            let secs = crate::timefmt::parse_sqlite(&updated).unwrap_or(0).max(0) as u64;
            Session { id, cwd, modified: SystemTime::UNIX_EPOCH + Duration::from_secs(secs), agent: kind }
        })
        .collect()
}
