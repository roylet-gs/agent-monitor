//! Harness plugins: everything specific to one coding-agent CLI.
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

pub mod antigravity;
pub mod claude;
pub mod codebuddy;
pub mod codex;
pub mod copilot;
pub mod cursor;
pub mod droid;
pub mod embedded;
pub mod gemini;
pub mod qoder;
pub mod grok;
pub mod muse;
pub mod qwen;
pub mod manifest;

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

    /// Whether a hook marks a session closed (`SessionEnd`). Without one,
    /// am-hook closes the owner's other settled sessions of this harness when
    /// a new one starts, so finished conversations don't pile up.
    fn ends_sessions(&self) -> bool {
        true
    }

    /// The key holding `event → entries` in the settings file; empty when the
    /// file is that map itself.
    fn hooks_key(&self) -> &str {
        "hooks"
    }

    /// Top-level keys the hook file needs besides `hooks` (e.g. Cursor's
    /// `"version": 1`), added when missing.
    fn hook_file_defaults(&self) -> serde_json::Map<String, Value> {
        serde_json::Map::new()
    }

    /// One entry for `hooks.<event>` in the settings file.
    /// `invocation` is the program part (`"/path/am-hook" hook-event`).
    fn hook_entry(&self, event: &str, invocation: &str, native: bool) -> Value;

    /// How am's hooks get into the CLI's config: merged into a JSON settings
    /// file (the default, driven by [`hooks_key`](Harness::hooks_key) and
    /// [`hook_entry`](Harness::hook_entry)), a block am owns in a TOML/YAML
    /// file, or a whole generated plugin file. See
    /// [`HookInstall`](crate::hooks_installer::HookInstall).
    fn hook_install(&self) -> crate::hooks_installer::HookInstall {
        crate::hooks_installer::HookInstall::Json
    }

    /// [`HookInstall::TextBlock`](crate::hooks_installer::HookInstall::TextBlock)
    /// only: where am's block goes when it can't simply stay where it is or
    /// be appended, e.g. nested under a YAML key the user's file already has
    /// (a second top-level key of that name would be a duplicate). `text` is
    /// the file without am's block; `block` is am's whole block as
    /// [`block_text`](crate::hooks_installer::block_text) renders it (marker
    /// lines included). Return the whole new file text, which must still hold
    /// the [`block_begin`](crate::hooks_installer::block_begin) /
    /// [`block_end`](crate::hooks_installer::block_end) lines (they may be
    /// indented, and the body re-indented or re-rendered from `ctx`), or
    /// `None` for the default (replaced in place, else
    /// [`append_block`](crate::hooks_installer::append_block)). An error
    /// refuses the install before anything is written, and `am doctor` shows
    /// it.
    fn place_hook_block(&self, _text: &str, _block: &str, _ctx: &crate::hooks_installer::HookContext) -> std::io::Result<Option<String>> {
        Ok(None)
    }

    /// State the CLI keeps beside its hook config, kept in step with the
    /// hooks in every install style: called with `Some(hooks)` on install,
    /// after the new config is worked out and just before it is written (an
    /// error stops the install with nothing written), and with `None` after
    /// uninstall (also when there was nothing to remove). Hermes's shell-hook
    /// allowlist. Default: nothing.
    fn hook_companion(&self, _ctx: Option<&crate::hooks_installer::HookContext>) -> std::io::Result<()> {
        Ok(())
    }

    /// The full shell command one hook runs for native `event`, given the
    /// program part (`"/path/am-hook" hook-event`). Used by the text-block
    /// and owned-file install styles; JSON entries come from
    /// [`hook_entry`](Harness::hook_entry).
    fn hook_command(&self, event: &str, invocation: &str) -> String {
        hook_command_line(self.id(), None, event, invocation)
    }

    /// How long the CLI should let one hook run, in milliseconds.
    fn hook_timeout_ms(&self) -> u64 {
        5_000
    }

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

    /// Transcript files modified at or after `since` that may hold token
    /// usage (subagent transcripts included). Feeds today's token total.
    fn usage_files(&self, _since: std::time::SystemTime) -> Vec<PathBuf> {
        Vec::new()
    }

    /// The token usage recorded on one transcript line, if any.
    fn usage_record(&self, _line: &str) -> Option<crate::usage::UsageRecord> {
        None
    }

    /// One line of the CLI's machine-readable output (e.g. a headless run's
    /// JSON event stream, as a review script prints it) as lines to show in
    /// the jobs pane: `Some(vec![])` drops it, `None` = not this CLI's.
    /// `cwd` is the script's directory (paths under it read relative).
    fn readable_output(&self, _line: &str, _cwd: Option<&str>) -> Option<Vec<String>> {
        None
    }

    /// Whether a process (name + argv) is this CLI. Its processes, and tools
    /// it spawns directly, are not shown as a worktree's running processes.
    fn matches_process(&self, _name: &str, _argv: &[String]) -> bool {
        false
    }

    // --- MCP ---------------------------------------------------------------------

    /// Where the CLI reads MCP server definitions, so `am mcp install` can
    /// register am's MCP server (`am mcp`); `None` when am can't.
    fn mcp_config(&self) -> Option<McpConfig> {
        None
    }

    /// argv that registers `entry` as MCP server `name` through the CLI itself,
    /// for a CLI that rewrites its config file while it runs; `None` edits
    /// [`mcp_config`](Harness::mcp_config)'s file instead.
    fn mcp_add_command(&self, _name: &str, _entry: &Value) -> Option<Vec<String>> {
        None
    }

    /// argv that removes MCP server `name` through the CLI; pairs with
    /// [`mcp_add_command`](Harness::mcp_add_command).
    fn mcp_remove_command(&self, _name: &str) -> Option<Vec<String>> {
        None
    }
}

/// An agent CLI's MCP server config: a JSON file with a `name → server` map.
#[derive(Debug, Clone, PartialEq)]
pub struct McpConfig {
    pub path: PathBuf,
    /// Key of the server map (`mcpServers`).
    pub key: String,
    /// Extra keys for am's server entry besides `command`/`args` (`{"type": "stdio"}`).
    pub entry: serde_json::Map<String, Value>,
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
    pub const GEMINI: HarnessKind = HarnessKind(&gemini::GEMINI);
    pub const CODEX: HarnessKind = HarnessKind(&codex::CODEX);
    pub const QWEN: HarnessKind = HarnessKind(&qwen::QWEN);
    pub const DROID: HarnessKind = HarnessKind(&droid::DROID);
    pub const COPILOT: HarnessKind = HarnessKind(&copilot::COPILOT);
    pub const CURSOR: HarnessKind = HarnessKind(&cursor::CURSOR);
    pub const QODER: HarnessKind = HarnessKind(&qoder::QODER);
    pub const CODEBUDDY: HarnessKind = HarnessKind(&codebuddy::CODEBUDDY);
    pub const ANTIGRAVITY: HarnessKind = HarnessKind(&antigravity::ANTIGRAVITY);
    pub const GROK: HarnessKind = HarnessKind(&grok::GROK);
    pub const MUSE: HarnessKind = HarnessKind(&muse::MUSE);

    pub fn provider(self) -> &'static dyn Harness {
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
pub const BUILTIN: &[HarnessKind] = &[
    HarnessKind::CLAUDE,
    HarnessKind::GEMINI,
    HarnessKind::CODEX,
    HarnessKind::QWEN,
    HarnessKind::DROID,
    HarnessKind::COPILOT,
    HarnessKind::CURSOR,
    HarnessKind::QODER,
    HarnessKind::CODEBUDDY,
    HarnessKind::ANTIGRAVITY,
    HarnessKind::GROK,
    HarnessKind::MUSE,
];

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
        crate::debug!("agents", "Loaded harness plugin {} from {}", p.manifest.id, p.source.display());
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

/// A script's output line as the first plugin that recognises it renders it
/// ([`Harness::readable_output`]); `None` shows the line as it is.
pub fn readable_output(line: &str, cwd: Option<&str>) -> Option<Vec<String>> {
    if !line.trim_start().starts_with('{') {
        return None;
    }
    all().iter().find_map(|k| k.provider().readable_output(line, cwd))
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

/// `{invocation} --agent <id> --worktree "$VAR"|"" --event <event>`: the
/// command line every non-Claude hook runs. Without a project-dir variable,
/// `--worktree ""` makes am-hook use the payload's `cwd`.
pub fn hook_command_line(id: &str, project_dir_var: Option<&str>, event: &str, invocation: &str) -> String {
    let worktree = project_dir_var.map(|v| format!("\"${v}\"")).unwrap_or_else(|| "\"\"".into());
    format!("{invocation} --agent {id} --worktree {worktree} --event {event}")
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
        for &k in BUILTIN {
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
