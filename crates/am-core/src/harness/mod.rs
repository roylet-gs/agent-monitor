//! Agent plugins: everything specific to one coding-agent CLI.
//!
//! am tracks agents through their CLI's hooks. Each provider knows how to
//! - install its hook entries into the CLI's own settings file,
//! - translate the CLI's native hook payload into am's canonical [`HookEvent`]
//!   (the vocabulary is Claude Code's: `UserPromptSubmit`, `PreToolUse`,
//!   `Stop`…, which is what `status::map_event_to_status` understands),
//! - find the CLI's saved sessions for a worktree and build the command that
//!   resumes one, and recognise the CLI's processes.
//!
//! Providers live in an open registry; nothing outside this module names a
//! specific agent. Built-ins are listed in [`BUILTIN`] (Claude Code; Gemini
//! CLI, itself a manifest plus custom session discovery). Users add more
//! without code: a JSON manifest in `<app dir>/agents/<id>.json`
//! ([`manifest`]). Plugins can be disabled with the `disabledAgents`
//! setting. See `docs/agent-providers.md`.

pub mod capture;
pub mod claude;
pub mod gemini;
pub mod issues;
pub mod lifecycle;
pub mod manifest;
pub mod replay;
pub mod training;
pub mod validate;

use std::path::{Path, PathBuf};
use std::sync::RwLock;

use serde::Deserialize;
use serde_json::Value;

use crate::launch::Session;
use crate::paths;
use crate::settings::Settings;
use crate::status::HookEvent;

/// Where a provider comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    BuiltIn,
    /// A manifest file in `<app dir>/agents/`.
    Plugin(PathBuf),
}

/// One agent CLI.
pub trait Harness: Sync + Send {
    /// Stable id: the `--agent` flag, the DB `agent` column, settings values.
    fn id(&self) -> &str;

    /// Human name, e.g. "Claude Code".
    fn name(&self) -> &str;

    /// Short label for status lines, e.g. "Claude".
    fn short_name(&self) -> &str;

    /// The executable that starts or resumes a session.
    fn program(&self) -> &str;

    fn origin(&self) -> Origin {
        Origin::BuiltIn
    }

    /// A caveat shown next to the hook status (`am doctor`, the plugin page).
    fn notes(&self) -> Option<&str> {
        None
    }

    /// The declarative manifest behind this harness, if it has one (manifest
    /// plugins, and built-ins written as one such as Gemini).
    fn manifest(&self) -> Option<&manifest::Manifest> {
        None
    }

    /// `draft` while a plugin is being trained. Built-ins are published.
    fn status(&self) -> manifest::PluginStatus {
        match (self.origin(), self.manifest()) {
            (Origin::Plugin(_), Some(m)) => m.status,
            _ => manifest::PluginStatus::Published,
        }
    }

    /// Whether this CLI has been used on this machine (its config dir exists;
    /// a bare `$PATH` hit isn't enough, as the CLI writes the dir on first
    /// run). Decides what `am hooks install` and the setup wizard install by
    /// default.
    fn is_present(&self) -> bool;

    // --- Hooks ---------------------------------------------------------------

    /// The CLI's user-level settings file that holds hook config.
    fn settings_path(&self) -> PathBuf;

    /// Native event names am registers a hook for.
    fn hook_events(&self) -> Vec<String>;

    /// One matcher entry for `hooks.<event>` in the settings file.
    /// `invocation` is the program part (`"/path/am-hook" hook-event`).
    fn hook_entry(&self, event: &str, invocation: &str, native: bool) -> Value;

    /// Translate a native hook payload into the canonical event.
    /// `event_override` is the `--event` flag (a native event name).
    fn normalize(&self, payload: &Value, event_override: Option<&str>) -> HookEvent;

    // --- Sessions & processes --------------------------------------------------

    /// Sessions started at `root` or any subdirectory, newest first.
    fn discover_sessions(&self, root: &Path) -> Vec<Session>;

    /// Arguments after [`program`](Harness::program): resume `session_id`,
    /// else continue the latest session (`continue_latest`), else start fresh.
    fn resume_args(&self, session_id: Option<&str>, continue_latest: bool) -> Vec<String>;

    /// How full the session's context window is, if the agent records it.
    /// Drives the context bar on a worktree row; `None` hides it.
    fn context_usage(&self, _root: &Path, _session_id: &str) -> Option<crate::context::ContextUsage> {
        None
    }

    /// Whether a process (name + argv) is this CLI. Its processes, and tools
    /// it spawns directly, are not shown as a worktree's running processes.
    fn matches_process(&self, _name: &str, _argv: &[String]) -> bool {
        false
    }
}

/// Recognise a CLI process by name, by the basename of one of its first two
/// non-flag argv entries (a node script: `node [flags] …/bin/mycli`), or by
/// an argv substring (an install path such as `/@acme/mycli/`).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessSpec {
    #[serde(default)]
    pub names: Vec<String>,
    #[serde(default)]
    pub argv_contains: Vec<String>,
}

impl ProcessSpec {
    pub fn matches(&self, name: &str, argv: &[String]) -> bool {
        let base = |s: &str| s.rsplit('/').next().unwrap_or(s).trim_start_matches('-').to_owned();
        if self.names.iter().any(|n| n == name) {
            return true;
        }
        let head = argv.iter().take(4);
        if head.clone().filter(|a| !a.starts_with("--")).take(2).any(|a| self.names.contains(&base(a))) {
            return true;
        }
        argv.iter().take(4).any(|a| self.argv_contains.iter().any(|c| a.contains(c.as_str())))
    }
}

/// A handle on a registered provider; cheap to copy, compared by id.
#[derive(Clone, Copy)]
pub struct HarnessKind(&'static dyn Harness);

impl HarnessKind {
    pub const CLAUDE: HarnessKind = HarnessKind(&claude::Claude);
    pub const GEMINI: HarnessKind = HarnessKind(&gemini::Gemini);

    pub fn provider(self) -> &'static dyn Harness {
        self.0
    }

    pub fn id(self) -> &'static str {
        self.0.id()
    }

    pub fn is_builtin(self) -> bool {
        self.0.origin() == Origin::BuiltIn
    }

    /// A handle on a provider that isn't registered (tests, previews). Leaks it.
    pub fn leak(provider: Box<dyn Harness>) -> HarnessKind {
        HarnessKind(Box::leak(provider))
    }

    pub fn is_draft(self) -> bool {
        self.0.status() == manifest::PluginStatus::Draft
    }

    /// Look up a registered agent. Built-ins are checked first so the hook
    /// path never reads the plugin directory for them.
    pub fn get(id: &str) -> Option<HarnessKind> {
        BUILTIN.iter().copied().find(|k| k.id() == id).or_else(|| all().iter().copied().find(|k| k.id() == id))
    }

    /// Lenient form for DB values and settings: unknown (a removed plugin) or
    /// missing → Claude, which is what NULL meant before plugins existed.
    pub fn from_db(s: Option<&str>) -> HarnessKind {
        s.and_then(HarnessKind::get).unwrap_or_default()
    }
}

impl Default for HarnessKind {
    fn default() -> Self {
        HarnessKind::CLAUDE
    }
}

impl PartialEq for HarnessKind {
    fn eq(&self, other: &Self) -> bool {
        self.id() == other.id()
    }
}

impl Eq for HarnessKind {}

impl std::hash::Hash for HarnessKind {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id().hash(state);
    }
}

impl std::fmt::Debug for HarnessKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HarnessKind({})", self.id())
    }
}

/// Compiled-in providers, in display order. To add one: a module here and
/// one entry in this list.
pub const BUILTIN: [HarnessKind; 2] = [HarnessKind::CLAUDE, HarnessKind::GEMINI];

struct Registry {
    agents: Vec<HarnessKind>,
    errors: Vec<String>,
}

static REGISTRY: RwLock<Option<&'static Registry>> = RwLock::new(None);

fn load_registry() -> &'static Registry {
    let taken: Vec<&str> = BUILTIN.iter().map(|k| k.id()).collect();
    let (plugins, errors) = manifest::load_dir(&plugins_dir(), &taken);
    let mut agents = BUILTIN.to_vec();
    for p in plugins {
        crate::debug!("agents", "Loaded agent plugin {} from {}", p.manifest.id, p.source.display());
        // Lives for the whole process, like the built-in unit structs.
        agents.push(HarnessKind(Box::leak(Box::new(p))));
    }
    // Leaked so handles stay `&'static`; a reload (a manifest changed on
    // disk) leaks the old one, which is small and rare.
    Box::leak(Box::new(Registry { agents, errors }))
}

fn registry() -> &'static Registry {
    if let Some(r) = *REGISTRY.read().unwrap_or_else(|e| e.into_inner()) {
        return r;
    }
    let mut slot = REGISTRY.write().unwrap_or_else(|e| e.into_inner());
    slot.get_or_insert_with(load_registry)
}

/// Re-read `<app dir>/agents/` (a plugin was added, edited or removed while
/// this process runs). Handles from before keep working; they compare by id.
pub fn reload() {
    let fresh = load_registry();
    *REGISTRY.write().unwrap_or_else(|e| e.into_inner()) = Some(fresh);
    crate::debug!("agents", "Reloaded plugins: {} registered", fresh.agents.len());
}

/// Newest modification time in the plugin dir (adding, changing or removing
/// a file bumps the dir or the file), to decide when to [`reload`].
pub fn plugins_stamp() -> Option<std::time::SystemTime> {
    let dir = plugins_dir();
    let mut newest = std::fs::metadata(&dir).and_then(|m| m.modified()).ok()?;
    for e in std::fs::read_dir(&dir).ok()?.flatten() {
        if let Ok(t) = e.metadata().and_then(|m| m.modified()) {
            newest = newest.max(t);
        }
    }
    Some(newest)
}

/// `<app dir>/agents`: one `<id>.json` manifest per plugin.
pub fn plugins_dir() -> PathBuf {
    paths::app_dir().join("agents")
}

/// Every registered agent: built-ins, then plugins (by file name). Plugins
/// are read once per process.
pub fn all() -> &'static [HarnessKind] {
    &registry().agents
}

/// Plugin files that couldn't be loaded, as `file: reason`.
pub fn plugin_errors() -> &'static [String] {
    &registry().errors
}

pub fn ids() -> Vec<&'static str> {
    all().iter().map(|k| k.id()).collect()
}

pub fn is_enabled(kind: HarnessKind, settings: &Settings) -> bool {
    !settings.disabled_agents.iter().any(|d| d == kind.id())
}

/// Agents not switched off in settings.
pub fn enabled(settings: &Settings) -> Vec<HarnessKind> {
    all().iter().copied().filter(|k| is_enabled(*k, settings)).collect()
}

/// Enabled agents that look installed; what `am hooks install` and the
/// wizard install by default. Claude is always included (when enabled): it is
/// am's original target and its hooks are harmless without Claude Code.
pub fn detected(settings: &Settings) -> Vec<HarnessKind> {
    enabled(settings).into_iter().filter(|k| *k == HarnessKind::CLAUDE || k.provider().is_present()).collect()
}

pub(crate) fn home_join(rel: &str) -> PathBuf {
    paths::home_dir().join(rel)
}

/// `child` is `root` or below it.
pub(crate) fn is_within(child: &str, root: &str) -> bool {
    child == root || child.strip_prefix(root).is_some_and(|rest| rest.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_are_registered_and_resolvable() {
        for k in BUILTIN {
            assert_eq!(HarnessKind::get(k.id()), Some(k));
            assert!(k.is_builtin());
        }
        assert_eq!(HarnessKind::from_db(None), HarnessKind::CLAUDE);
        assert_eq!(HarnessKind::from_db(Some("nope")), HarnessKind::CLAUDE);
        assert_eq!(HarnessKind::from_db(Some("gemini")), HarnessKind::GEMINI);
    }

    #[test]
    fn disabled_agents_drop_out() {
        let s = Settings::parse(r#"{"disabledAgents":["gemini"]}"#);
        assert!(!is_enabled(HarnessKind::GEMINI, &s));
        assert!(enabled(&s).contains(&HarnessKind::CLAUDE));
        assert!(!enabled(&s).contains(&HarnessKind::GEMINI));
        assert!(!detected(&s).contains(&HarnessKind::GEMINI));
    }

    #[test]
    fn process_spec_matching() {
        let spec = ProcessSpec { names: vec!["gemini".into(), "gemini.js".into()], argv_contains: vec!["/@google/gemini-cli/".into()] };
        let argv = |s: &str| s.split(' ').map(String::from).collect::<Vec<_>>();
        assert!(spec.matches("gemini", &[]));
        assert!(spec.matches("node", &argv("node /u/.nvm/bin/gemini")));
        assert!(spec.matches("node", &argv("node --max-old-space-size=8192 /n/x/bundle/gemini.js")));
        assert!(spec.matches("node", &argv("node /n/lib/node_modules/@google/gemini-cli/dist/index.js")));
        assert!(!spec.matches("node", &argv("node server.js gemini")));
        assert!(!spec.matches("vite", &argv("vite --port 3000")));
    }
}
