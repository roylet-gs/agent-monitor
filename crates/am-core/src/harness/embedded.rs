//! A built-in harness written as a manifest: the same JSON format users drop
//! into `<app dir>/agents/`, compiled in with `include_str!`, plus optional
//! Rust session discovery for CLIs whose session store needs code. Without
//! it, the sessions am's own hooks recorded are the ones offered for resume.
//! Every built-in except Claude Code is one of these (see `BUILTIN`). A CLI
//! whose hook config isn't a JSON file picks another [`HookInstall`] style
//! with [`Embedded::with_hook_install`].
//!
//! The `with_*` builders cover what a manifest can't say, so a built-in
//! never needs its own `Harness` impl wrapping an `Embedded`:
//! - [`with_hook_file`](Embedded::with_hook_file): the hook file (and key)
//!   picked at runtime (`$XDG_CONFIG_HOME`, `$KIRO_HOME`…).
//! - [`with_presence`](Embedded::with_presence): detection beyond the
//!   manifest's `configDir` (a config dir an env var moves).
//! - [`with_normalize`](Embedded::with_normalize): a payload pass around the
//!   manifest's mapping (key spellings, events to drop or reinterpret).
//! - [`with_hook_install`](Embedded::with_hook_install),
//!   [`with_block_placement`](Embedded::with_block_placement),
//!   [`with_hook_companion`](Embedded::with_hook_companion): non-JSON hook
//!   config and the state kept beside it.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::{Duration, SystemTime};

use serde_json::{Map, Value};

use super::{Harness, HarnessKind, is_within};
use super::manifest::{Manifest, expand_home};
use crate::hooks_installer::{HookCompanion, HookContext, HookInstall, PlaceBlock};
use crate::launch::Session;
use crate::status::HookEvent;

pub struct Embedded {
    manifest: LazyLock<Manifest>,
    discover: Option<fn(&Path) -> Vec<Session>>,
    /// Picks the hook file and its hooks key at runtime, for a CLI that reads
    /// one of several files (Factory Droid).
    hook_file: Option<fn() -> (PathBuf, &'static str)>,
    /// How the hooks are installed when not merged into a JSON file.
    hook_install: Option<fn() -> HookInstall>,
    /// Also present when this says so ([`Embedded::with_presence`]).
    present: Option<fn() -> bool>,
    /// The payload pass ([`Embedded::with_normalize`]).
    normalize: Option<NormalizeFn>,
    /// Where a text block goes ([`Harness::place_hook_block`]).
    place_block: Option<PlaceBlock>,
    /// State kept beside the hook config ([`Harness::hook_companion`]).
    companion: Option<HookCompanion>,
}

/// [`Embedded::with_normalize`]: `(manifest, payload, --event) → event`.
/// Call `manifest.normalize(payload, event_override)` for the declarative
/// part, on a rewritten payload and/or adjusting what it returns.
pub type NormalizeFn = fn(&Manifest, &Value, Option<&str>) -> HookEvent;

impl Embedded {
    /// `load` parses the embedded JSON (`|| Manifest::builtin(include_str!(…))`).
    pub const fn new(load: fn() -> Manifest, discover: Option<fn(&Path) -> Vec<Session>>) -> Embedded {
        Embedded {
            manifest: LazyLock::new(load),
            discover,
            hook_file: None,
            hook_install: None,
            present: None,
            normalize: None,
            place_block: None,
            companion: None,
        }
    }

    pub const fn with_hook_file(mut self, f: fn() -> (PathBuf, &'static str)) -> Embedded {
        self.hook_file = Some(f);
        self
    }

    /// Install the hooks with a [`HookInstall`] style other than the JSON
    /// merge (a TOML/YAML block, a generated plugin file). The file is the
    /// manifest's `hooks.settingsPath`.
    pub const fn with_hook_install(mut self, f: fn() -> HookInstall) -> Embedded {
        self.hook_install = Some(f);
        self
    }

    /// Count the CLI as present when `f` says so, as well as when the
    /// manifest's `configDir` exists: for a config dir an environment
    /// variable moves (`$OMP_AGENT_DIR`, `$GROK_HOME`), or a runtime-picked
    /// hook file's directory (`|| hook_file().0.parent().is_some_and(Path::is_dir)`).
    pub const fn with_presence(mut self, f: fn() -> bool) -> Embedded {
        self.present = Some(f);
        self
    }

    /// Normalise payloads through `f` instead of the manifest alone, for a
    /// rule the manifest's maps can't express: rewrite the payload before
    /// `manifest.normalize` (camelCase keys), adjust the event after it (a
    /// `Stop` that isn't the end of the turn), or drop it: with
    /// `--worktree ""`, an event with no `cwd` is skipped by am-hook
    /// (`HookEvent { event: "ignored".into(), ..HookEvent::default() }`).
    pub const fn with_normalize(mut self, f: NormalizeFn) -> Embedded {
        self.normalize = Some(f);
        self
    }

    /// Place the [`HookInstall::TextBlock`] somewhere other than in place /
    /// at the end of the file ([`Harness::place_hook_block`]).
    pub const fn with_block_placement(mut self, f: PlaceBlock) -> Embedded {
        self.place_block = Some(f);
        self
    }

    /// Keep state beside the hook config in step with the hooks
    /// ([`Harness::hook_companion`]).
    pub const fn with_hook_companion(mut self, f: HookCompanion) -> Embedded {
        self.companion = Some(f);
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
        self.manifest.config_dir.as_deref().is_some_and(|d| expand_home(d).is_dir()) || self.present.is_some_and(|f| f())
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

    fn hook_install(&self) -> HookInstall {
        self.hook_install.map(|f| f()).unwrap_or(HookInstall::Json)
    }

    fn place_hook_block(&self, text: &str, block: &str, ctx: &HookContext) -> std::io::Result<Option<String>> {
        match self.place_block {
            Some(f) => f(text, block, ctx),
            None => Ok(None),
        }
    }

    fn hook_companion(&self, ctx: Option<&HookContext>) -> std::io::Result<()> {
        match self.companion {
            Some(f) => f(ctx),
            None => Ok(()),
        }
    }

    fn hook_command(&self, event: &str, invocation: &str) -> String {
        self.manifest.hook_command(event, invocation)
    }

    fn hook_timeout_ms(&self) -> u64 {
        self.manifest.hooks.timeout_ms
    }

    fn normalize(&self, payload: &Value, event_override: Option<&str>) -> HookEvent {
        match self.normalize {
            Some(f) => f(&self.manifest, payload, event_override),
            None => self.manifest.normalize(payload, event_override),
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks_installer::{TextBlock, block_text};
    use serde_json::json;

    fn load() -> Manifest {
        Manifest::builtin(
            r#"{"id":"emb","name":"Emb","program":"emb","configDir":"/nonexistent/am-embedded-test",
                "hooks":{"settingsPath":"/nonexistent/am-embedded-test/config.yaml"},
                "events":{"done":"Stop","go":"UserPromptSubmit"},
                "resume":{"session":["--resume","{id}"],"start":["chat","--agent","am"]}}"#,
        )
    }

    static PLAIN: Embedded = Embedded::new(load, None);

    static BUILT: Embedded = Embedded::new(load, None)
        .with_presence(|| true)
        .with_normalize(|m, payload, event| {
            // camelCase keys read as snake_case, and an internal event dropped.
            let mut p = payload.clone();
            if let Some(id) = payload.get("sessionId") {
                p["session_id"] = id.clone();
            }
            if payload.get("internal").is_some() {
                return HookEvent { event: "ignored".into(), ..HookEvent::default() };
            }
            m.normalize(&p, event)
        })
        .with_hook_install(|| HookInstall::TextBlock(TextBlock { comment: "#", render: |_| "hooks:\n".into() }))
        .with_block_placement(|text, block, ctx| Ok(Some(format!("{text}# {}\n{block}", ctx.agent_id))))
        .with_hook_companion(|ctx| match ctx {
            Some(c) if c.hooks.len() != 2 => Err(std::io::Error::other("expected two hooks")),
            _ => Ok(()),
        });

    #[test]
    fn presence_adds_to_the_config_dir_check() {
        assert!(!PLAIN.is_present(), "the manifest's configDir doesn't exist");
        assert!(BUILT.is_present(), "`with_presence` says so");
    }

    #[test]
    fn normalize_wraps_the_manifest_mapping() {
        let e = BUILT.normalize(&json!({"hook_event_name": "done", "sessionId": "s1"}), None);
        assert_eq!((e.event.as_str(), e.session_id.as_deref()), ("Stop", Some("s1")));
        assert_eq!(PLAIN.normalize(&json!({"hook_event_name": "done", "sessionId": "s1"}), None).session_id, None);
        let e = BUILT.normalize(&json!({"internal": true}), Some("done"));
        assert_eq!((e.event.as_str(), e.cwd, e.session_id), ("ignored", None, None));
    }

    #[test]
    fn placement_and_companion_are_delegated() {
        let ctx = HookContext::new(&BUILT, None);
        let block = block_text("#", "emb", "hooks:\n");
        assert_eq!(BUILT.place_hook_block("a: 1\n", &block, &ctx).unwrap().as_deref(), Some(format!("a: 1\n# emb\n{block}").as_str()));
        assert_eq!(PLAIN.place_hook_block("a: 1\n", &block, &ctx).unwrap(), None);
        assert!(BUILT.hook_companion(Some(&ctx)).is_ok());
        assert!(BUILT.hook_companion(None).is_ok());
        let one = HookContext { hooks: ctx.hooks[..1].to_vec(), ..ctx };
        assert!(BUILT.hook_companion(Some(&one)).is_err());
        assert!(PLAIN.hook_companion(Some(&one)).is_ok());
    }

    #[test]
    fn resume_start_is_the_fresh_session() {
        assert_eq!(BUILT.resume_args(None, false), ["chat", "--agent", "am"]);
        assert_eq!(BUILT.resume_args(Some("x"), false), ["--resume", "x"]);
    }
}
