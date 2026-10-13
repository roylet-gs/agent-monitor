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

pub mod claude;
pub mod gemini;
pub mod manifest;

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

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
pub trait AgentProvider: Sync + Send {
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

    /// Arguments after [`program`](AgentProvider::program): resume `session_id`,
    /// else continue the latest session (`continue_latest`), else start fresh.
    fn resume_args(&self, session_id: Option<&str>, continue_latest: bool) -> Vec<String>;

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
pub struct AgentKind(&'static dyn AgentProvider);

impl AgentKind {
    pub const CLAUDE: AgentKind = AgentKind(&claude::Claude);
    pub const GEMINI: AgentKind = AgentKind(&gemini::Gemini);

    pub fn provider(self) -> &'static dyn AgentProvider {
        self.0
    }

    pub fn id(self) -> &'static str {
        self.0.id()
    }

    pub fn is_builtin(self) -> bool {
        self.0.origin() == Origin::BuiltIn
    }

    /// Look up a registered agent. Built-ins are checked first so the hook
    /// path never reads the plugin directory for them.
    pub fn get(id: &str) -> Option<AgentKind> {
        BUILTIN.iter().copied().find(|k| k.id() == id).or_else(|| all().iter().copied().find(|k| k.id() == id))
    }

    /// Lenient form for DB values and settings: unknown (a removed plugin) or
    /// missing → Claude, which is what NULL meant before plugins existed.
    pub fn from_db(s: Option<&str>) -> AgentKind {
        s.and_then(AgentKind::get).unwrap_or_default()
    }
}

impl Default for AgentKind {
    fn default() -> Self {
        AgentKind::CLAUDE
    }
}

impl PartialEq for AgentKind {
    fn eq(&self, other: &Self) -> bool {
        self.id() == other.id()
    }
}

impl Eq for AgentKind {}

impl std::hash::Hash for AgentKind {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id().hash(state);
    }
}

impl std::fmt::Debug for AgentKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "AgentKind({})", self.id())
    }
}

/// Compiled-in providers, in display order. To add one: a module here and
/// one entry in this list.
pub const BUILTIN: [AgentKind; 2] = [AgentKind::CLAUDE, AgentKind::GEMINI];

struct Registry {
    agents: Vec<AgentKind>,
    errors: Vec<String>,
}

fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let taken: Vec<&str> = BUILTIN.iter().map(|k| k.id()).collect();
        let (plugins, errors) = manifest::load_dir(&plugins_dir(), &taken);
        let mut agents = BUILTIN.to_vec();
        for p in plugins {
            crate::debug!("agents", "Loaded agent plugin {} from {}", p.manifest.id, p.source.display());
            // Lives for the whole process, like the built-in unit structs.
            agents.push(AgentKind(Box::leak(Box::new(p))));
        }
        Registry { agents, errors }
    })
}

/// `<app dir>/agents`: one `<id>.json` manifest per plugin.
pub fn plugins_dir() -> PathBuf {
    paths::app_dir().join("agents")
}

/// Every registered agent: built-ins, then plugins (by file name). Plugins
/// are read once per process.
pub fn all() -> &'static [AgentKind] {
    &registry().agents
}

/// Plugin files that couldn't be loaded, as `file: reason`.
pub fn plugin_errors() -> &'static [String] {
    &registry().errors
}

pub fn ids() -> Vec<&'static str> {
    all().iter().map(|k| k.id()).collect()
}

pub fn is_enabled(kind: AgentKind, settings: &Settings) -> bool {
    !settings.disabled_agents.iter().any(|d| d == kind.id())
}

/// Agents not switched off in settings.
pub fn enabled(settings: &Settings) -> Vec<AgentKind> {
    all().iter().copied().filter(|k| is_enabled(*k, settings)).collect()
}

/// Enabled agents that look installed; what `am hooks install` and the
/// wizard install by default. Claude is always included (when enabled): it is
/// am's original target and its hooks are harmless without Claude Code.
pub fn detected(settings: &Settings) -> Vec<AgentKind> {
    enabled(settings).into_iter().filter(|k| *k == AgentKind::CLAUDE || k.provider().is_present()).collect()
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
            assert_eq!(AgentKind::get(k.id()), Some(k));
            assert!(k.is_builtin());
        }
        assert_eq!(AgentKind::from_db(None), AgentKind::CLAUDE);
        assert_eq!(AgentKind::from_db(Some("nope")), AgentKind::CLAUDE);
        assert_eq!(AgentKind::from_db(Some("gemini")), AgentKind::GEMINI);
    }

    #[test]
    fn disabled_agents_drop_out() {
        let s = Settings::parse(r#"{"disabledAgents":["gemini"]}"#);
        assert!(!is_enabled(AgentKind::GEMINI, &s));
        assert!(enabled(&s).contains(&AgentKind::CLAUDE));
        assert!(!enabled(&s).contains(&AgentKind::GEMINI));
        assert!(!detected(&s).contains(&AgentKind::GEMINI));
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
