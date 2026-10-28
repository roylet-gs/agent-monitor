//! Hook config in each agent CLI's settings file (`~/.claude/settings.json`,
//! `~/.gemini/settings.json`, `~/.cursor/hooks.json`…). Every file keeps its
//! hooks under a top-level `hooks` object keyed by event; the entry shapes
//! (Claude's nested `{matcher, hooks: [{command}]}` or a flat `{command}`)
//! come from the providers in `crate::harness`; the rules are shared and ported from the TS
//! `src/lib/hooks-installer.ts`: replace our entries, keep everyone else's, so
//! the TS and Rust installers can run against each other's output.
//!
//! A CLI whose hook config isn't a JSON file picks another [`HookInstall`]
//! style: a [`TextBlock`] am owns inside a TOML/YAML file, or an
//! [`OwnedFile`] (a generated plugin) am owns whole. See "Hook install
//! styles" in `docs/agent-providers.md`.

use std::path::{Path, PathBuf};

use crate::harness::{Harness, HarnessKind};
use crate::{debug, info, paths, warn};
use serde_json::{Map, Value};

/// Whether a hook command is one of ours. Superset of the TS marker
/// `/\bam(-hook"?)?\s+hook-event\b/`: also matches `am-rs` and a quoted
/// `"…/am" hook-event`, so uninstall cleans up every variant we ever wrote.
pub fn is_am_hook_command(command: &str) -> bool {
    let bytes = command.as_bytes();
    let word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut from = 0;
    while let Some(off) = command[from..].find("hook-event") {
        let at = from + off;
        from = at + "hook-event".len();
        // `hook-event` must end at a word boundary…
        if bytes.get(from).is_some_and(|b| word(*b)) {
            continue;
        }
        // …and be preceded by whitespace…
        let before = command[..at].trim_end_matches([' ', '\t']);
        if before.len() == at {
            continue;
        }
        // …then an optional closing quote and the program name.
        let before = before.strip_suffix('"').unwrap_or(before);
        for name in ["am-hook", "am-rs", "am"] {
            if let Some(rest) = before.strip_suffix(name)
                && !rest.as_bytes().last().is_some_and(|b| word(*b))
            {
                return true;
            }
        }
    }
    false
}

pub(crate) fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// A cargo build output (`…/target/debug/am-hook`, `…/target/<triple>/release/…`):
/// usually inside a worktree that gets deleted, so hooks shouldn't stay pinned to it.
fn is_cargo_build_output(bin: &Path) -> bool {
    let dirs: Vec<_> = bin.parent().into_iter().flat_map(Path::components).map(|c| c.as_os_str()).collect();
    match dirs.split_last() {
        Some((last, above)) => (*last == "debug" || *last == "release") && above.iter().any(|d| *d == "target"),
        None => false,
    }
}

/// The native `am-hook` binary to point hooks at: `$AM_HOOK_BIN`, then
/// [`find_installed_binary`]. `$AM_NO_NATIVE_HOOK` opts out.
pub fn find_native_hook_binary() -> Option<PathBuf> {
    if std::env::var_os("AM_NO_NATIVE_HOOK").is_some_and(|v| !v.is_empty()) {
        return None;
    }
    if let Some(p) = std::env::var_os("AM_HOOK_BIN").filter(|p| !p.is_empty()).map(PathBuf::from)
        && is_executable(&p)
    {
        return Some(p);
    }
    find_installed_binary("am-hook")
}

/// An am binary (`am-hook`, `am`) for a config file to run later: the one
/// next to this executable (a `cargo install`/release puts them side by
/// side), then `$PATH`, then `~/.cargo/bin`.
///
/// Config outlives the binary that wrote it, so two cases prefer a stable
/// path: a sibling that `$PATH` reaches through a symlink (Homebrew's
/// `bin/am-hook` → the versioned `Cellar/…` path `current_exe` resolves to,
/// which `brew upgrade` removes) is written as the `$PATH` entry, and a
/// sibling in a cargo `target/` dir (a dev build) is used only when no other
/// copy is installed.
pub fn find_installed_binary(name: &str) -> Option<PathBuf> {
    let sibling = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join(name)))
        .filter(|p| is_executable(p));
    let mut installed: Vec<PathBuf> = Vec::new();
    if let Some(path) = std::env::var_os("PATH") {
        installed.extend(std::env::split_paths(&path).filter(|d| !d.as_os_str().is_empty()).map(|d| d.join(name)));
    }
    installed.push(paths::home_dir().join(".cargo").join("bin").join(name));
    installed.retain(|p| is_executable(p));

    if let Some(sib) = sibling {
        let real = std::fs::canonicalize(&sib).ok();
        if let Some(alias) = installed.iter().find(|p| p != &&sib && real.is_some() && std::fs::canonicalize(p).ok() == real) {
            return Some(alias.clone());
        }
        if !is_cargo_build_output(&sib) {
            return Some(sib);
        }
        if let Some(stable) = installed.iter().find(|p| !is_cargo_build_output(p)) {
            debug!("hooks", "Skipping dev build {} for installed {}", sib.display(), stable.display());
            return Some(stable.clone());
        }
        return Some(sib);
    }
    installed.into_iter().next()
}

/// The native binary an am hook command runs (`"/path/am-hook" hook-event …`),
/// `None` for the `cat | am hook-event` CLI form.
fn native_program(command: &str) -> Option<PathBuf> {
    let quoted = command.strip_prefix('"')?;
    Some(PathBuf::from(&quoted[..quoted.find('"')?]))
}

/// The missing binaries `agent`'s installed hooks point at (deleted worktree,
/// upgraded-away install). Every hook then fails with "No such file or directory".
pub fn dangling_hook_binaries(agent: HarnessKind) -> Vec<PathBuf> {
    dangling_provider(agent.provider())
}

fn dangling_provider(p: &dyn Harness) -> Vec<PathBuf> {
    match p.hook_install() {
        HookInstall::Json => dangling_in(&read_settings(&p.settings_path()).unwrap_or_default(), p.hooks_key()),
        HookInstall::TextBlock(tb) => {
            let text = read_text(&p.settings_path()).unwrap_or_default();
            match find_block(&text, &block_begin(tb.comment, p.id()), &block_end(tb.comment, p.id())) {
                Ok(Some((s, e))) => missing_programs(&text[s..e]),
                _ => Vec::new(),
            }
        }
        HookInstall::OwnedFile(_) => match read_text(&p.settings_path()) {
            Ok(text) if text.contains(OWNED_FILE_MARKER) => missing_programs(&text),
            _ => Vec::new(),
        },
    }
}

/// The missing binaries among the hook programs written in a block of text
/// (a TOML/YAML block or a generated plugin file).
fn missing_programs(text: &str) -> Vec<PathBuf> {
    let mut missing: Vec<PathBuf> = hook_programs_in_text(text).into_iter().filter(|p| !is_executable(p)).collect();
    missing.sort();
    missing.dedup();
    missing
}

/// Absolute program paths in text am generated: a quoted (or escaped-quoted,
/// `\"/abs/am-hook\"` inside a TOML/YAML string) absolute path that is named
/// like an am binary or is followed by `hook-event`, also inside the same
/// string (`"/abs/am-hook hook-event …"`) or as the next element of an argv
/// array (`["/abs/my-hook", "hook-event", …]`, a plugin's
/// [`HookContext::argv`] as JSON, even split across lines). Paths with spaces
/// work when the path is quoted on its own.
pub fn hook_programs_in_text(text: &str) -> Vec<PathBuf> {
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
    let mut found = Vec::new();
    for (i, _) in text.match_indices(['"', '\'']) {
        let rest = &text[i + 1..];
        if !rest.starts_with('/') {
            continue;
        }
        let mut end = rest.find(['"', '\'', '\\', '\n']).unwrap_or(rest.len());
        if let Some(at) = rest[..end].find(" hook-event") {
            end = at;
        }
        let (path, after) = rest.split_at(end);
        let after = after.trim_start_matches(['"', '\'', '\\']).trim_start_matches([' ', '\t']);
        // The next element of an argv array: `"/abs/x", "hook-event"`.
        let after = match after.strip_prefix(',') {
            Some(next) => next.trim_start_matches([' ', '\t', '\r', '\n']).trim_start_matches(['"', '\'']),
            None => after,
        };
        let named = matches!(path.rsplit('/').next(), Some("am-hook" | "am-rs" | "am"));
        let runs_hook = after.strip_prefix("hook-event").is_some_and(|r| !r.starts_with(word));
        if named || runs_hook {
            found.push(PathBuf::from(path));
        }
    }
    found
}

fn dangling_in(settings: &Map<String, Value>, key: &str) -> Vec<PathBuf> {
    let mut missing: Vec<PathBuf> = hooks_of(settings, key)
        .values()
        .filter_map(Value::as_array)
        .flatten()
        .flat_map(entry_commands)
        .filter(|c| is_am_hook_command(c))
        .filter_map(native_program)
        .filter(|p| !is_executable(p))
        .collect();
    missing.sort();
    missing.dedup();
    missing
}

/// Reinstall the hooks of every agent whose am hooks point at a missing
/// binary, so they find the current one. Returns the agents repaired.
pub fn repair_dangling_hooks() -> Vec<HarnessKind> {
    let mut repaired = Vec::new();
    for &kind in crate::harness::all() {
        let missing = dangling_hook_binaries(kind);
        if missing.is_empty() {
            continue;
        }
        let gone = missing.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ");
        match install_hooks(kind) {
            Ok(_) => {
                info!("hooks", "Repaired {} hooks that pointed at missing {gone}", kind.provider().name());
                repaired.push(kind);
            }
            Err(e) => warn!("hooks", "{} hooks point at missing {gone}; reinstall failed: {e}", kind.provider().name()),
        }
    }
    repaired
}

/// The program part of a hook command: the native binary when there is one,
/// else the `am` CLI (`cat |` shim kept from the TS app).
fn invocation(native_bin: Option<&Path>) -> String {
    match native_bin {
        Some(bin) => format!("\"{}\" hook-event", bin.display()),
        None => "cat | am hook-event".to_string(),
    }
}

pub fn build_hook_entry(agent: HarnessKind, event: &str, native_bin: Option<&Path>) -> Value {
    agent.provider().hook_entry(event, &invocation(native_bin), native_bin.is_some())
}

/// The settings file as a JSON object. A missing file is empty; a file that
/// exists but isn't a JSON object (Gemini allows comments) is an error, so we
/// never overwrite what we couldn't read.
pub fn read_settings(path: &Path) -> std::io::Result<Map<String, Value>> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Map::new()),
        Err(e) => return Err(e),
    };
    if text.trim().is_empty() {
        return Ok(Map::new());
    }
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(m)) => Ok(m),
        Ok(_) => Err(std::io::Error::other(format!("{} is not a JSON object; not modifying it", path.display()))),
        Err(e) => Err(std::io::Error::other(format!(
            "can't parse {} ({e}); remove comments or fix it, then retry. Not modifying it",
            path.display()
        ))),
    }
}

/// Written in place (not temp + rename) so a symlinked settings file, common
/// with dotfile managers, stays a symlink.
pub fn write_settings(path: &Path, settings: &Map<String, Value>) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut text = serde_json::to_string_pretty(settings).map_err(std::io::Error::other)?;
    text.push('\n');
    std::fs::write(path, text)
}

/// Keys a hook command can sit under (`bash` is Copilot CLI's).
const COMMAND_KEYS: [&str; 2] = ["command", "bash"];

/// The commands in one entry of `hooks.<event>`: nested (`{matcher, hooks:
/// [{command}]}`, Claude's layout) or flat (`{command}`, Cursor's).
fn entry_commands(entry: &Value) -> Vec<&str> {
    let nested = entry.get("hooks").and_then(Value::as_array).into_iter().flatten();
    std::iter::once(entry)
        .chain(nested)
        .flat_map(|h| COMMAND_KEYS.iter().filter_map(move |k| h.get(*k).and_then(Value::as_str)))
        .collect()
}

fn matcher_is_ours(m: &Value) -> bool {
    entry_commands(m).into_iter().any(is_am_hook_command)
}

fn without_am_hooks(matchers: &[Value]) -> Vec<Value> {
    matchers.iter().filter(|m| !matcher_is_ours(m)).cloned().collect()
}

/// The event → entries map: under `key`, or the whole file when `key` is
/// empty (Factory Droid's `hooks.json` is keyed by event at the top level).
fn hooks_of(settings: &Map<String, Value>, key: &str) -> Map<String, Value> {
    if key.is_empty() {
        return settings.clone();
    }
    match settings.get(key) {
        Some(Value::Object(h)) => h.clone(),
        _ => Map::new(),
    }
}

fn put_hooks(settings: &mut Map<String, Value>, key: &str, hooks: Map<String, Value>) {
    if key.is_empty() {
        *settings = hooks;
    } else if hooks.is_empty() {
        settings.shift_remove(key);
    } else {
        settings.insert(key.into(), Value::Object(hooks));
    }
}

/// Install `agent`'s hooks. Returns the binary they now point at (`None` =
/// the `am hook-event` CLI fallback).
pub fn install_hooks(agent: HarnessKind) -> std::io::Result<Option<PathBuf>> {
    install_provider(agent.provider())
}

fn install_provider(provider: &dyn Harness) -> std::io::Result<Option<PathBuf>> {
    let native = find_native_hook_binary();
    match provider.hook_install() {
        HookInstall::Json => install_json(provider, native),
        style => install_text(provider, &style, native),
    }
}

fn install_json(provider: &dyn Harness, native: Option<PathBuf>) -> std::io::Result<Option<PathBuf>> {
    let path = provider.settings_path();
    let mut settings = read_settings(&path)?;
    let mut hooks = hooks_of(&settings, provider.hooks_key());
    // Replace (rather than skip) existing am entries so re-running install
    // switches between the TS command and the native binary.
    for event in provider.hook_events() {
        let existing = hooks.get(event.as_str()).and_then(Value::as_array).cloned().unwrap_or_default();
        let mut next = without_am_hooks(&existing);
        next.push(provider.hook_entry(&event, &invocation(native.as_deref()), native.is_some()));
        hooks.insert(event, Value::Array(next));
    }
    put_hooks(&mut settings, provider.hooks_key(), hooks);
    // Keys the file format needs (Cursor/Copilot: `"version": 1`); never
    // overwrites a value the user or the CLI already wrote.
    for (k, v) in provider.hook_file_defaults() {
        settings.entry(k).or_insert(v);
    }
    provider.hook_companion(Some(&HookContext::new(provider, native.as_deref())))?;
    write_settings(&path, &settings)?;
    info!(
        "hooks",
        "Installed {} hooks into {} using {}",
        provider.name(),
        path.display(),
        native.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "am hook-event (TS)".into())
    );
    Ok(native)
}

pub fn uninstall_hooks(agent: HarnessKind) -> std::io::Result<()> {
    uninstall_provider(agent.provider())
}

fn uninstall_provider(provider: &dyn Harness) -> std::io::Result<()> {
    match provider.hook_install() {
        HookInstall::Json => uninstall_json(provider)?,
        HookInstall::TextBlock(tb) => uninstall_text_block(provider, &tb)?,
        HookInstall::OwnedFile(of) => uninstall_owned_file(provider, &of)?,
    }
    // Once the hooks are gone, also when there were none to remove.
    provider.hook_companion(None)
}

fn uninstall_json(provider: &dyn Harness) -> std::io::Result<()> {
    let path = provider.settings_path();
    if !path.exists() {
        return Ok(());
    }
    let mut settings = read_settings(&path)?;
    let mut hooks = hooks_of(&settings, provider.hooks_key());
    for event in provider.hook_events() {
        let Some(existing) = hooks.get(event.as_str()).and_then(Value::as_array).cloned() else { continue };
        let kept = without_am_hooks(&existing);
        if kept.is_empty() {
            hooks.shift_remove(event.as_str());
        } else {
            hooks.insert(event.to_string(), Value::Array(kept));
        }
    }
    put_hooks(&mut settings, provider.hooks_key(), hooks);
    write_settings(&path, &settings)?;
    info!("hooks", "Uninstalled {} hooks from {}", provider.name(), path.display());
    Ok(())
}

pub fn is_hooks_installed(agent: HarnessKind) -> bool {
    is_installed_provider(agent.provider())
}

fn is_installed_provider(provider: &dyn Harness) -> bool {
    match provider.hook_install() {
        HookInstall::Json => {
            let hooks = hooks_of(&read_settings(&provider.settings_path()).unwrap_or_default(), provider.hooks_key());
            provider.hook_events().iter().any(|e| hooks.get(e.as_str()).and_then(Value::as_array).is_some_and(|ms| ms.iter().any(matcher_is_ours)))
        }
        HookInstall::TextBlock(tb) => {
            let text = read_text(&provider.settings_path()).unwrap_or_default();
            find_block(&text, &block_begin(tb.comment, provider.id()), &block_end(tb.comment, provider.id())).is_ok_and(|b| b.is_some())
        }
        HookInstall::OwnedFile(_) => read_text(&provider.settings_path()).is_ok_and(|t| t.contains(OWNED_FILE_MARKER)),
    }
}

/// Whether am can read (and so safely edit) `agent`'s hook config: a JSON
/// file that parses, a text file whose am block (if any) is complete, a
/// plugin file that is am's (or absent). `am doctor` shows the error.
pub fn check_hook_config(agent: HarnessKind) -> std::io::Result<()> {
    check_provider(agent.provider())
}

fn check_provider(p: &dyn Harness) -> std::io::Result<()> {
    let path = p.settings_path();
    match p.hook_install() {
        HookInstall::Json => read_settings(&path).map(drop),
        // Placing the block also catches a file the harness can't put it in
        // (`Harness::place_hook_block` refusing).
        HookInstall::TextBlock(tb) => place_text_block(p, &tb, &read_text(&path)?, &HookContext::new(p, None)).map(drop),
        HookInstall::OwnedFile(of) => {
            if path.exists() && !read_text(&path)?.contains(OWNED_FILE_MARKER) {
                return Err(not_ours(&path));
            }
            match &of.register {
                Some(r) => read_settings(&r.path).map(drop),
                None => Ok(()),
            }
        }
    }
}

// --- Text-block and owned-file styles ----------------------------------------

/// How a harness's hooks get into its CLI's config ([`Harness::hook_install`]).
/// The file is always [`Harness::settings_path`].
#[derive(Debug, Clone)]
pub enum HookInstall {
    /// Merge entries into a JSON settings file under
    /// [`Harness::hooks_key`], one [`Harness::hook_entry`] per event,
    /// replacing only am's entries. Every CLI whose config is JSON.
    Json,
    /// A block am owns inside a text config file (TOML, YAML…), between
    /// `<comment> agent-monitor hooks <id> begin` / `… end` lines.
    TextBlock(TextBlock),
    /// A whole file am owns, e.g. a generated JS/TS plugin that spawns
    /// am-hook. It must contain [`OWNED_FILE_MARKER`].
    OwnedFile(OwnedFile),
}

impl HookInstall {
    /// For log lines: `json`, `text block`, `owned file`.
    pub fn style(&self) -> &'static str {
        match self {
            HookInstall::Json => "json",
            HookInstall::TextBlock(_) => "text block",
            HookInstall::OwnedFile(_) => "owned file",
        }
    }
}

/// [`HookInstall::TextBlock`]: am's block in a TOML/YAML file.
#[derive(Debug, Clone)]
pub struct TextBlock {
    /// The format's line-comment prefix (`#` for TOML and YAML).
    pub comment: &'static str,
    /// The block's body, between the marker lines: the hooks in the file's
    /// own syntax. Quote values with [`toml_string`] / [`yaml_string`].
    /// Where the block goes is [`Harness::place_hook_block`] (default: where
    /// it already is, else appended).
    pub render: fn(&HookContext) -> String,
}

/// [`Harness::place_hook_block`] as a function, for
/// `Embedded::with_block_placement`: `(text without am's block, am's whole
/// block, hooks) → Some(new file text) | None (default placement)`.
pub type PlaceBlock = fn(text: &str, block: &str, ctx: &HookContext) -> std::io::Result<Option<String>>;

/// [`Harness::hook_companion`] as a function, for
/// `Embedded::with_hook_companion`: `Some(hooks)` on install, `None` on
/// uninstall.
pub type HookCompanion = fn(Option<&HookContext>) -> std::io::Result<()>;

/// [`HookInstall::OwnedFile`]: a file am writes whole and deletes on
/// uninstall (only while it still holds [`OWNED_FILE_MARKER`]).
#[derive(Debug, Clone)]
pub struct OwnedFile {
    /// The file's full text; must contain [`OWNED_FILE_MARKER`] (in a comment).
    pub render: fn(&HookContext) -> String,
    /// A JSON registration the CLI also needs (OpenCode's `"plugin": [...]`).
    pub register: Option<JsonListEntry>,
}

/// `value` kept in the JSON array at top-level `key` of the JSON file `path`:
/// added on install when missing, removed on uninstall (the key goes with
/// its last value). The file is read and written like a JSON hook file
/// ([`read_settings`]), so one am can't parse is left alone.
#[derive(Debug, Clone)]
pub struct JsonListEntry {
    pub path: PathBuf,
    pub key: String,
    pub value: Value,
}

/// Marker an [`OwnedFile`] must carry; am only overwrites or deletes a file
/// that holds it.
pub const OWNED_FILE_MARKER: &str = "agent-monitor-hook-plugin v1";

/// One hook as the text styles render it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookCommand {
    /// The CLI's native event name.
    pub event: String,
    /// The full shell command ([`Harness::hook_command`]): `"/abs/am-hook"
    /// hook-event --agent <id> --worktree "$VAR"|"" --event <event>`.
    pub command: String,
    pub timeout_ms: u64,
}

impl HookCommand {
    /// The timeout in whole seconds, rounded up.
    pub fn timeout_secs(&self) -> u64 {
        self.timeout_ms.div_ceil(1000)
    }
}

/// What a [`TextBlock`] or [`OwnedFile`] render function gets.
#[derive(Debug, Clone)]
pub struct HookContext {
    pub agent_id: String,
    /// The program hooks run: the native `am-hook`'s absolute path, or `am`
    /// (the CLI fallback, which also reads the payload on stdin).
    pub program: String,
    /// Whether [`program`](HookContext::program) is the native binary.
    pub native: bool,
    /// The shell form of the program part: `"/abs/am-hook" hook-event` or
    /// `cat | am hook-event`.
    pub invocation: String,
    /// One per [`Harness::hook_events`], in that order.
    pub hooks: Vec<HookCommand>,
}

impl HookContext {
    pub fn new(provider: &dyn Harness, native: Option<&Path>) -> HookContext {
        let invocation = invocation(native);
        let timeout_ms = provider.hook_timeout_ms();
        HookContext {
            agent_id: provider.id().to_owned(),
            program: native.map(|p| p.display().to_string()).unwrap_or_else(|| "am".into()),
            native: native.is_some(),
            hooks: provider
                .hook_events()
                .into_iter()
                .map(|event| HookCommand { command: provider.hook_command(&event, &invocation), event, timeout_ms })
                .collect(),
            invocation,
        }
    }

    /// argv that runs one hook without a shell (a plugin's `spawn`), the
    /// payload going to its stdin: `[program, "hook-event", "--agent", id,
    /// "--worktree", worktree, "--event", event]`. An empty `worktree`
    /// makes am-hook use the payload's `cwd`.
    pub fn argv(&self, event: &str, worktree: &str) -> Vec<String> {
        [self.program.as_str(), "hook-event", "--agent", &self.agent_id, "--worktree", worktree, "--event", event]
            .map(str::to_owned)
            .to_vec()
    }
}

/// The line that opens am's block in a text config file.
pub fn block_begin(comment: &str, id: &str) -> String {
    format!("{comment} agent-monitor hooks {id} begin")
}

/// The line that closes am's block.
pub fn block_end(comment: &str, id: &str) -> String {
    format!("{comment} agent-monitor hooks {id} end")
}

/// A TOML basic string (`"…"`) holding `s`, e.g. a hook command with its own
/// double quotes.
pub fn toml_string(s: &str) -> String {
    double_quoted(s)
}

/// A YAML double-quoted scalar holding `s` (safe for `#`, `:`, quotes…).
pub fn yaml_string(s: &str) -> String {
    double_quoted(s)
}

/// A JS/TS string literal holding `s` (a JSON string, which JS accepts).
pub fn js_string(s: &str) -> String {
    Value::from(s).to_string()
}

/// `"…"` with the escapes TOML basic strings and YAML double-quoted scalars
/// share: `\\`, `\"`, `\t`, `\n`, `\r`, and `\uXXXX` for other controls.
fn double_quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A text file's contents; missing = empty.
fn read_text(path: &Path) -> std::io::Result<String> {
    match std::fs::read_to_string(path) {
        Ok(t) => Ok(t),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e),
    }
}

/// Written in place, like [`write_settings`], so a symlink stays one.
fn write_text(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, text)
}

fn not_ours(path: &Path) -> std::io::Error {
    std::io::Error::other(format!("{} exists but wasn't written by am (no `{OWNED_FILE_MARKER}` marker); not touching it", path.display()))
}

/// Byte range of am's block (from the start of the `begin` line to just
/// after the `end` line). A `begin` with no `end` after it is an error, so
/// a hand-damaged block is never half-replaced.
fn find_block(text: &str, begin: &str, end: &str) -> std::io::Result<Option<(usize, usize)>> {
    let mut at = 0;
    let mut start = None;
    for line in text.split_inclusive('\n') {
        let next = at + line.len();
        let bare = line.trim();
        match start {
            None if bare == begin => start = Some(at),
            Some(s) if bare == end => return Ok(Some((s, next))),
            _ => {}
        }
        at = next;
    }
    match start {
        Some(_) => Err(std::io::Error::other(format!("`{begin}` has no matching `{end}` line; fix or remove am's block, then retry"))),
        None => Ok(None),
    }
}

/// `text` with am's block set to `body`: replaced where it is, else
/// appended after a blank line. Everything outside the block is kept.
pub fn upsert_block(text: &str, comment: &str, id: &str, body: &str) -> std::io::Result<String> {
    let block = block_text(comment, id, body);
    Ok(match find_block(text, &block_begin(comment, id), &block_end(comment, id))? {
        Some((s, e)) => format!("{}{block}{}", &text[..s], &text[e..]),
        None => append_block(text, &block),
    })
}

/// am's whole block: the [`block_begin`] line, `body`, the [`block_end`]
/// line, ending in a newline.
pub fn block_text(comment: &str, id: &str, body: &str) -> String {
    let nl = if body.is_empty() || body.ends_with('\n') { "" } else { "\n" };
    format!("{}\n{body}{nl}{}\n", block_begin(comment, id), block_end(comment, id))
}

/// `text` with `block` appended after a blank line (just `block` when
/// `text` is empty): the default placement.
pub fn append_block(text: &str, block: &str) -> String {
    if text.is_empty() {
        return block.to_owned();
    }
    format!("{text}{}\n{block}", if text.ends_with('\n') { "" } else { "\n" })
}

/// `text` with am's block set to what `tb` renders for `ctx`: where the
/// harness puts it ([`Harness::place_hook_block`], given the text without
/// the old block), else replaced in place or appended ([`upsert_block`]).
/// A placement that loses the marker lines is refused, so the hooks can
/// always be found and removed again.
fn place_text_block(provider: &dyn Harness, tb: &TextBlock, text: &str, ctx: &HookContext) -> std::io::Result<String> {
    let id = provider.id();
    let body = (tb.render)(ctx);
    let without = remove_block(text, tb.comment, id)?;
    let Some(placed) = provider.place_hook_block(without.as_deref().unwrap_or(text), &block_text(tb.comment, id, &body), ctx)? else {
        return upsert_block(text, tb.comment, id, &body);
    };
    let (begin, end) = (block_begin(tb.comment, id), block_end(tb.comment, id));
    if find_block(&placed, &begin, &end)?.is_none() {
        return Err(std::io::Error::other(format!("{id}: the placed hook block lacks its `{begin}` / `{end}` lines")));
    }
    Ok(placed)
}

/// `text` without am's block (`None` when there is none). A block at the end
/// takes the blank line [`upsert_block`] put before it.
pub fn remove_block(text: &str, comment: &str, id: &str) -> std::io::Result<Option<String>> {
    let Some((s, e)) = find_block(text, &block_begin(comment, id), &block_end(comment, id))? else { return Ok(None) };
    let (mut before, after) = (&text[..s], &text[e..]);
    if after.is_empty() && before.ends_with("\n\n") {
        before = &before[..before.len() - 1];
    }
    Ok(Some(format!("{before}{after}")))
}

fn install_text(provider: &dyn Harness, style: &HookInstall, native: Option<PathBuf>) -> std::io::Result<Option<PathBuf>> {
    let path = provider.settings_path();
    let ctx = HookContext::new(provider, native.as_deref());
    match style {
        HookInstall::Json => unreachable!("JSON hooks go through install_json"),
        HookInstall::TextBlock(tb) => {
            let text = place_text_block(provider, tb, &read_text(&path)?, &ctx)?;
            provider.hook_companion(Some(&ctx))?;
            write_text(&path, &text)?;
        }
        HookInstall::OwnedFile(of) => {
            if path.exists() && !read_text(&path)?.contains(OWNED_FILE_MARKER) {
                return Err(not_ours(&path));
            }
            let text = (of.render)(&ctx);
            if !text.contains(OWNED_FILE_MARKER) {
                return Err(std::io::Error::other(format!("{}'s plugin file lacks the `{OWNED_FILE_MARKER}` marker", provider.id())));
            }
            // Read the registration first so an unreadable file stops the
            // install before anything is written.
            let registry = of.register.as_ref().map(|r| read_settings(&r.path).map(|s| (r, s))).transpose()?;
            provider.hook_companion(Some(&ctx))?;
            write_text(&path, &text)?;
            if let Some((r, mut settings)) = registry {
                let list = settings.entry(r.key.clone()).or_insert_with(|| Value::Array(Vec::new()));
                let Value::Array(items) = list else {
                    return Err(std::io::Error::other(format!("`{}` in {} is not a list; not modifying it", r.key, r.path.display())));
                };
                if !items.contains(&r.value) {
                    items.push(r.value.clone());
                    write_settings(&r.path, &settings)?;
                    info!("hooks", "Registered {} in {} `{}`", r.value, r.path.display(), r.key);
                }
            }
        }
    }
    info!(
        "hooks",
        "Installed {} hooks into {} ({}) using {}",
        provider.name(),
        path.display(),
        style.style(),
        native.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "am hook-event".into())
    );
    Ok(native)
}

fn uninstall_text_block(provider: &dyn Harness, tb: &TextBlock) -> std::io::Result<()> {
    let path = provider.settings_path();
    if !path.exists() {
        return Ok(());
    }
    if let Some(text) = remove_block(&read_text(&path)?, tb.comment, provider.id())? {
        write_text(&path, &text)?;
        info!("hooks", "Uninstalled {} hooks from {} (text block)", provider.name(), path.display());
    }
    Ok(())
}

fn uninstall_owned_file(provider: &dyn Harness, of: &OwnedFile) -> std::io::Result<()> {
    let path = provider.settings_path();
    if let Some(r) = &of.register
        && r.path.exists()
    {
        let mut settings = read_settings(&r.path)?;
        if let Some(Value::Array(items)) = settings.get_mut(&r.key)
            && items.contains(&r.value)
        {
            items.retain(|v| v != &r.value);
            if items.is_empty() {
                settings.shift_remove(&r.key);
            }
            write_settings(&r.path, &settings)?;
        }
    }
    if !path.exists() {
        return Ok(());
    }
    if !read_text(&path)?.contains(OWNED_FILE_MARKER) {
        warn!("hooks", "Not deleting {}: it lacks am's `{OWNED_FILE_MARKER}` marker", path.display());
        return Ok(());
    }
    std::fs::remove_file(&path)?;
    info!("hooks", "Uninstalled {} hooks: removed {} (owned file)", provider.name(), path.display());
    Ok(())
}

/// Display form of a provider's settings path (`~/.gemini/settings.json`).
pub fn display_path(p: &dyn Harness) -> String {
    tilde(&p.settings_path())
}

/// A path with the home directory shown as `~`.
pub fn tilde(path: &Path) -> String {
    match path.strip_prefix(paths::home_dir()) {
        Ok(rel) => format!("~/{}", rel.display()),
        Err(_) => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_matches_every_variant_we_write() {
        assert!(is_am_hook_command("cat | am hook-event --worktree \"$CLAUDE_PROJECT_DIR\" --event Stop"));
        assert!(is_am_hook_command("\"/Users/x/.cargo/bin/am-hook\" hook-event --worktree x"));
        assert!(is_am_hook_command("/usr/local/bin/am-hook hook-event"));
        assert!(is_am_hook_command("\"/opt/am-rs\" hook-event --event Stop"));
        assert!(is_am_hook_command("am hook-event"));
    }

    #[test]
    fn marker_ignores_other_hooks() {
        assert!(!is_am_hook_command("echo hello"));
        assert!(!is_am_hook_command("cam hook-event"));
        assert!(!is_am_hook_command("am hook-events"));
        assert!(!is_am_hook_command("my-hook-event"));
        assert!(!is_am_hook_command("program hook-event"));
    }

    #[test]
    fn cargo_build_outputs_are_recognised() {
        assert!(is_cargo_build_output(Path::new("/r/.claude/worktrees/x/target/debug/am-hook")));
        assert!(is_cargo_build_output(Path::new("/r/target/aarch64-apple-darwin/release/am-hook")));
        assert!(!is_cargo_build_output(Path::new("/Users/x/.cargo/bin/am-hook")));
        assert!(!is_cargo_build_output(Path::new("/opt/homebrew/Cellar/agent-monitor/2.1.6/bin/am-hook")));
        assert!(!is_cargo_build_output(Path::new("/srv/debug/am-hook")));
    }

    #[test]
    fn native_program_reads_the_quoted_path() {
        assert_eq!(native_program("\"/a b/am-hook\" hook-event --event Stop"), Some(PathBuf::from("/a b/am-hook")));
        assert_eq!(native_program("cat | am hook-event --event Stop"), None);
    }

    #[test]
    fn dangling_finds_only_our_missing_binaries() {
        let dir = tempfile::tempdir().unwrap();
        let live = dir.path().join("am-hook");
        std::fs::write(&live, "#!/bin/sh\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&live, std::fs::Permissions::from_mode(0o755)).unwrap();
        let cmd = |bin: &str, ev: &str| serde_json::json!({"hooks": [{"type": "command", "command": format!("\"{bin}\" hook-event --event {ev}")}]});
        let settings: Map<String, Value> = serde_json::from_value(serde_json::json!({"hooks": {
            "Stop": [cmd("/gone/am-hook", "Stop"), {"hooks": [{"type": "command", "command": "\"/gone/other-tool\" run"}]}],
            "SessionEnd": [cmd("/gone/am-hook", "SessionEnd")],
            "Notification": [cmd(&live.display().to_string(), "Notification"), {"hooks": [{"type": "command", "command": "cat | am hook-event"}]}],
        }}))
        .unwrap();
        assert_eq!(dangling_in(&settings, "hooks"), [PathBuf::from("/gone/am-hook")]);
        assert!(dangling_in(&Map::new(), "hooks").is_empty());
    }

    /// A Cursor-style flat hook file, installed next to someone else's hook.
    #[test]
    fn flat_layout_installs_idempotently_and_keeps_foreign_hooks() {
        use crate::harness::manifest::{Manifest, ManifestProvider};
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("hooks.json");
        let manifest = Manifest::parse(&format!(
            r#"{{"id":"flatcli","name":"Flat CLI","program":"flatcli",
                "hooks":{{"settingsPath":"{}","projectDirVar":"FLAT_DIR","layout":"flat","timeoutUnit":"s",
                          "file":{{"version":1}}}},
                "events":{{"stop":"Stop","preToolUse":"PreToolUse"}}}}"#,
            file.display()
        ))
        .unwrap();
        let p = ManifestProvider { manifest, source: dir.path().join("flatcli.json") };
        std::fs::write(&file, r#"{"version":1,"hooks":{"stop":[{"command":"./notify.sh"}]}}"#).unwrap();

        install_provider(&p).unwrap();
        let once = std::fs::read_to_string(&file).unwrap();
        install_provider(&p).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), once, "reinstalling changes nothing");
        let v: Value = serde_json::from_str(&once).unwrap();
        assert_eq!(v["version"], 1);
        let stop = v["hooks"]["stop"].as_array().unwrap();
        assert_eq!(stop.len(), 2);
        assert_eq!(stop[0]["command"], "./notify.sh");
        assert_eq!(stop[1]["timeout"], 5);
        assert!(stop[1].get("hooks").is_none() && stop[1].get("type").is_none());
        assert!(stop[1]["command"].as_str().unwrap().ends_with("--agent flatcli --worktree \"$FLAT_DIR\" --event stop"));
        assert!(is_installed_provider(&p));

        uninstall_provider(&p).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(v, serde_json::json!({"version":1,"hooks":{"stop":[{"command":"./notify.sh"}]}}));
        assert!(!is_installed_provider(&p));
    }

    #[test]
    fn a_fresh_flat_file_gets_its_version_key() {
        use crate::harness::manifest::{Manifest, ManifestProvider};
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("sub/am.json");
        let manifest = Manifest::parse(&format!(
            r#"{{"id":"cop","name":"Cop","program":"cop",
                "hooks":{{"settingsPath":"{}","layout":"flat","commandKey":"bash","timeoutKey":"timeoutSec",
                          "timeoutUnit":"s","entry":{{"type":"command"}},"file":{{"version":1}}}},
                "events":{{"Stop":"Stop"}}}}"#,
            file.display()
        ))
        .unwrap();
        let p = ManifestProvider { manifest, source: dir.path().join("cop.json") };
        install_provider(&p).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(v["version"], 1);
        let e = &v["hooks"]["Stop"][0];
        assert_eq!((e["type"].as_str(), e["timeoutSec"].as_u64()), (Some("command"), Some(5)));
        // No project-dir variable: am-hook falls back to the payload's cwd.
        assert!(e["bash"].as_str().unwrap().ends_with("--agent cop --worktree \"\" --event Stop"), "{e}");
        assert!(is_installed_provider(&p));
    }

    #[test]
    fn unparseable_settings_are_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        assert!(read_settings(&p).unwrap().is_empty());
        std::fs::write(&p, "{ // comment\n \"a\": 1 }").unwrap();
        assert!(read_settings(&p).is_err());
        std::fs::write(&p, "[1]").unwrap();
        assert!(read_settings(&p).is_err());
        std::fs::write(&p, "{\"b\":1,\"a\":2}").unwrap();
        let m = read_settings(&p).unwrap();
        write_settings(&p, &m).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "{\n  \"b\": 1,\n  \"a\": 2\n}\n");
    }
    // --- Text-block and owned-file styles --------------------------------------

    /// A test-only harness with a chosen install style and hook file.
    struct Dummy {
        path: PathBuf,
        install: HookInstall,
        place: Option<PlaceBlock>,
        companion: Option<HookCompanion>,
    }

    impl Harness for Dummy {
        fn id(&self) -> &str {
            "dummy"
        }
        fn name(&self) -> &str {
            "Dummy"
        }
        fn short_name(&self) -> &str {
            "Dummy"
        }
        fn program(&self) -> &str {
            "dummy"
        }
        fn is_present(&self) -> bool {
            true
        }
        fn settings_path(&self) -> PathBuf {
            self.path.clone()
        }
        fn hook_events(&self) -> Vec<String> {
            vec!["session_start".into(), "stop".into()]
        }
        fn hook_entry(&self, _event: &str, _invocation: &str, _native: bool) -> Value {
            Value::Null
        }
        fn hook_install(&self) -> HookInstall {
            self.install.clone()
        }
        fn place_hook_block(&self, text: &str, block: &str, ctx: &HookContext) -> std::io::Result<Option<String>> {
            self.place.map_or(Ok(None), |f| f(text, block, ctx))
        }
        fn hook_companion(&self, ctx: Option<&HookContext>) -> std::io::Result<()> {
            self.companion.map_or(Ok(()), |f| f(ctx))
        }
        fn hook_command(&self, event: &str, invocation: &str) -> String {
            crate::harness::hook_command_line("dummy", Some("DUMMY_DIR"), event, invocation)
        }
        fn hook_timeout_ms(&self) -> u64 {
            1_500
        }
        fn normalize(&self, _payload: &Value, _event_override: Option<&str>) -> crate::status::HookEvent {
            crate::status::HookEvent::from_json(&Value::Null, None)
        }
        fn discover_sessions(&self, _root: &Path) -> Vec<crate::launch::Session> {
            Vec::new()
        }
        fn resume_args(&self, _session_id: Option<&str>, _continue_latest: bool) -> Vec<String> {
            Vec::new()
        }
    }

    /// Kimi-style `[[hooks]]` tables.
    fn render_toml(ctx: &HookContext) -> String {
        ctx.hooks
            .iter()
            .map(|h| format!("[[hooks]]\nevent = {}\ncommand = {}\ntimeout = {}\n", toml_string(&h.event), toml_string(&h.command), h.timeout_secs()))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn render_js(ctx: &HookContext) -> String {
        format!(
            "// {OWNED_FILE_MARKER}: generated by agent-monitor; `am hooks uninstall` removes it.\nconst ARGV = {};\nexport default ARGV;\n",
            js_string(&ctx.argv("stop", "").join(" "))
        )
    }

    fn toml_dummy(path: PathBuf) -> Dummy {
        Dummy { path, install: HookInstall::TextBlock(TextBlock { comment: "#", render: render_toml }), place: None, companion: None }
    }

    fn js_dummy(path: PathBuf, register: Option<JsonListEntry>) -> Dummy {
        Dummy { path, install: HookInstall::OwnedFile(OwnedFile { render: render_js, register }), place: None, companion: None }
    }

    fn executable(dir: &Path, name: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let p = dir.join(name);
        std::fs::write(&p, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    #[test]
    fn quoting_helpers_escape_a_hook_command() {
        let cmd = "\"/a b/am-hook\" hook-event --agent k --worktree \"$K_DIR\" --event stop";
        let q = r#""\"/a b/am-hook\" hook-event --agent k --worktree \"$K_DIR\" --event stop""#;
        assert_eq!(toml_string(cmd), q);
        assert_eq!(yaml_string(cmd), q);
        assert_eq!(toml_string("a\\b\tc\nd\u{1}"), r#""a\\b\tc\nd\u0001""#);
        assert_eq!(js_string(cmd), q);
    }

    #[test]
    fn text_block_appends_replaces_and_removes_only_its_block() {
        let body = "a = 1\n";
        // Missing / empty file: just the block.
        let fresh = upsert_block("", "#", "k", body).unwrap();
        assert_eq!(fresh, "# agent-monitor hooks k begin\na = 1\n# agent-monitor hooks k end\n");
        assert_eq!(remove_block(&fresh, "#", "k").unwrap().as_deref(), Some(""));

        // Appended after a blank line, even without a trailing newline.
        let user = "[ui]\ntheme = \"dark\"";
        let with = upsert_block(user, "#", "k", body).unwrap();
        assert_eq!(with, "[ui]\ntheme = \"dark\"\n\n# agent-monitor hooks k begin\na = 1\n# agent-monitor hooks k end\n");
        assert_eq!(remove_block(&with, "#", "k").unwrap().as_deref(), Some("[ui]\ntheme = \"dark\"\n"));

        // Replaced in place, keeping what is around it (and another agent's block).
        let around = "x = 1\n# agent-monitor hooks k begin\nold = true\n# agent-monitor hooks k end\n# agent-monitor hooks j begin\nj = 1\n# agent-monitor hooks j end\ny = 2\n";
        let replaced = upsert_block(around, "#", "k", "a = 1").unwrap();
        assert_eq!(replaced, around.replace("old = true", "a = 1"));
        assert_eq!(upsert_block(&replaced, "#", "k", "a = 1").unwrap(), replaced, "idempotent");
        assert_eq!(
            remove_block(&replaced, "#", "k").unwrap().as_deref(),
            Some("x = 1\n# agent-monitor hooks j begin\nj = 1\n# agent-monitor hooks j end\ny = 2\n")
        );
        assert_eq!(remove_block("x = 1\n", "#", "k").unwrap(), None);

        // A begin with no end is refused rather than half-replaced.
        let broken = "# agent-monitor hooks k begin\na = 1\n";
        assert!(upsert_block(broken, "#", "k", body).is_err());
        assert!(remove_block(broken, "#", "k").is_err());
    }

    #[test]
    fn text_block_style_installs_into_a_toml_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("kimi/config.toml");
        let p = toml_dummy(path.clone());
        assert!(!is_installed_provider(&p));
        uninstall_provider(&p).unwrap();
        assert!(!path.exists(), "uninstalling with no file creates nothing");

        // File (and dir) missing: created.
        let bin = executable(dir.path(), "am-hook");
        install_text(&p, &p.hook_install(), Some(bin.clone())).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let cmd = format!("\"{}\" hook-event --agent dummy --worktree \"$DUMMY_DIR\" --event stop", bin.display());
        assert!(text.contains(&format!("command = {}\ntimeout = 2\n", toml_string(&cmd))), "{text}");
        assert!(text.starts_with("# agent-monitor hooks dummy begin\n[[hooks]]\nevent = \"session_start\"\n"), "{text}");
        assert!(is_installed_provider(&p));
        assert!(check_provider(&p).is_ok());
        assert!(dangling_provider(&p).is_empty());

        // The user's settings around it survive a reinstall and the uninstall.
        let user = format!("model = \"k2\"\n\n{text}\n[ui]\ntheme = \"dark\"\n");
        std::fs::write(&path, &user).unwrap();
        install_text(&p, &p.hook_install(), Some(bin.clone())).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), user, "reinstalling changes nothing");
        uninstall_provider(&p).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "model = \"k2\"\n\n\n[ui]\ntheme = \"dark\"\n");
        assert!(!is_installed_provider(&p));

        // Hooks pointing at a binary that's gone are found inside the block only.
        std::fs::write(&path, "cmd = \"\\\"/elsewhere/am-hook\\\" hook-event\"\n").unwrap();
        install_text(&p, &p.hook_install(), Some(PathBuf::from("/gone dir/am-hook"))).unwrap();
        assert_eq!(dangling_provider(&p), [PathBuf::from("/gone dir/am-hook")]);

        std::fs::write(&path, "# agent-monitor hooks dummy begin\n").unwrap();
        assert!(check_provider(&p).is_err());
        assert!(install_text(&p, &p.hook_install(), Some(bin)).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "# agent-monitor hooks dummy begin\n");
    }

    /// Hermes-style YAML: a top-level `hooks:` map of events.
    fn render_yaml(ctx: &HookContext) -> String {
        let mut out = String::from("hooks:\n");
        for h in &ctx.hooks {
            out.push_str(&format!("  {}:\n    - command: {}\n", h.event, yaml_string(&h.command)));
        }
        out
    }

    /// Nests am's block (minus its own `hooks:` line) under the user's
    /// top-level `hooks:`; refuses an inline `hooks: {}`; `None` without one.
    fn place_under_hooks(text: &str, block: &str, _ctx: &HookContext) -> std::io::Result<Option<String>> {
        let lines: Vec<&str> = text.split_inclusive('\n').collect();
        let Some(at) = lines.iter().position(|l| l.starts_with("hooks:")) else { return Ok(None) };
        if lines[at].trim_end() != "hooks:" {
            return Err(std::io::Error::other("inline `hooks:` value"));
        }
        let nested: String = block.lines().filter(|l| *l != "hooks:").map(|l| if l.starts_with('#') { format!("  {l}\n") } else { format!("{l}\n") }).collect();
        Ok(Some(format!("{}{nested}{}", lines[..=at].concat(), lines[at + 1..].concat())))
    }

    /// Calls to [`record_companion`]: the number of hooks, `None` = uninstall.
    static COMPANION_CALLS: std::sync::Mutex<Vec<Option<usize>>> = std::sync::Mutex::new(Vec::new());

    fn record_companion(ctx: Option<&HookContext>) -> std::io::Result<()> {
        COMPANION_CALLS.lock().unwrap().push(ctx.map(|c| c.hooks.len()));
        Ok(())
    }

    fn companion_calls() -> Vec<Option<usize>> {
        std::mem::take(&mut *COMPANION_CALLS.lock().unwrap())
    }

    #[test]
    fn a_text_block_can_be_placed_inside_the_users_key_with_a_companion() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hermes/config.yaml");
        let p = Dummy {
            path: path.clone(),
            install: HookInstall::TextBlock(TextBlock { comment: "#", render: render_yaml }),
            place: Some(place_under_hooks),
            companion: Some(record_companion),
        };
        let bin = executable(dir.path(), "am-hook");
        companion_calls();

        // No `hooks:` of the user's: the default placement (appended).
        install_text(&p, &p.hook_install(), Some(bin.clone())).unwrap();
        let alone = std::fs::read_to_string(&path).unwrap();
        assert!(alone.starts_with("# agent-monitor hooks dummy begin\nhooks:\n  session_start:\n"), "{alone}");
        uninstall_provider(&p).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
        assert_eq!(companion_calls(), [Some(2), None], "the companion follows install and uninstall");

        // The user's `hooks:` key: am's events go inside it, between indented markers.
        let user = "model: x\nhooks:\n  mine:\n    - command: echo hi\nui:\n  theme: dark\n";
        std::fs::write(&path, user).unwrap();
        install_text(&p, &p.hook_install(), Some(bin.clone())).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let cmd = |e: &str| yaml_string(&format!("\"{}\" hook-event --agent dummy --worktree \"$DUMMY_DIR\" --event {e}", bin.display()));
        assert_eq!(
            text,
            format!(
                "model: x\nhooks:\n  # agent-monitor hooks dummy begin\n  session_start:\n    - command: {}\n  stop:\n    - command: {}\n  # agent-monitor hooks dummy end\n  mine:\n    - command: echo hi\nui:\n  theme: dark\n",
                cmd("session_start"),
                cmd("stop")
            )
        );
        assert!(is_installed_provider(&p));
        assert!(check_provider(&p).is_ok());
        assert!(dangling_provider(&p).is_empty());
        install_text(&p, &p.hook_install(), Some(bin.clone())).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text, "reinstalling re-places it identically");
        uninstall_provider(&p).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), user, "uninstall gives the user's file back");
        assert_eq!(companion_calls(), [Some(2), Some(2), None]);

        // A placement that refuses stops the install before anything (the companion too) is written.
        std::fs::write(&path, "hooks: {}\n").unwrap();
        assert!(install_text(&p, &p.hook_install(), Some(bin.clone())).is_err());
        assert!(check_provider(&p).is_err(), "am doctor shows the refusal");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hooks: {}\n");
        assert_eq!(companion_calls(), []);

        // A placement that drops the marker lines is refused (the hooks could never be removed).
        let lossy = Dummy { place: Some(|t, _, _| Ok(Some(format!("{t}hooks: {{}}\n")))), companion: None, ..p };
        std::fs::write(&path, "").unwrap();
        let err = install_text(&lossy, &lossy.hook_install(), Some(bin)).unwrap_err();
        assert!(err.to_string().contains("lacks its `# agent-monitor hooks dummy begin`"), "{err}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
    }

    #[test]
    fn owned_file_style_writes_and_removes_only_its_own_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plugins/agent-monitor.js");
        let p = js_dummy(path.clone(), None);
        let bin = executable(dir.path(), "am-hook");
        install_text(&p, &p.hook_install(), Some(bin.clone())).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains(OWNED_FILE_MARKER));
        assert!(text.contains(&format!("\"{} hook-event --agent dummy --worktree  --event stop\"", bin.display())), "{text}");
        assert!(is_installed_provider(&p));
        assert!(dangling_provider(&p).is_empty());
        install_text(&p, &p.hook_install(), Some(bin.clone())).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text, "idempotent");

        uninstall_provider(&p).unwrap();
        assert!(!path.exists());
        assert!(!is_installed_provider(&p));
        uninstall_provider(&p).unwrap();

        // Someone else's file of that name is neither overwritten nor deleted.
        std::fs::write(&path, "// mine\n").unwrap();
        assert!(install_text(&p, &p.hook_install(), Some(bin)).is_err());
        assert!(check_provider(&p).is_err());
        assert!(!is_installed_provider(&p));
        uninstall_provider(&p).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "// mine\n");
    }

    #[test]
    fn owned_file_finds_a_dangling_binary() {
        let dir = tempfile::tempdir().unwrap();
        let p = js_dummy(dir.path().join("am.ts"), None);
        install_text(&p, &p.hook_install(), Some(PathBuf::from("/gone/target/debug/am-hook"))).unwrap();
        assert_eq!(dangling_provider(&p), [PathBuf::from("/gone/target/debug/am-hook")]);
        // The CLI fallback names no binary.
        install_text(&p, &p.hook_install(), None).unwrap();
        assert!(dangling_provider(&p).is_empty());
    }

    #[test]
    fn owned_file_registers_itself_in_a_json_list() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plugin/am.js");
        let config = dir.path().join("opencode.json");
        std::fs::write(&config, r#"{"theme":"x","plugin":["other"]}"#).unwrap();
        let entry = || Some(JsonListEntry { path: config.clone(), key: "plugin".into(), value: Value::from(path.display().to_string()) });
        let p = js_dummy(path.clone(), entry());
        install_text(&p, &p.hook_install(), None).unwrap();
        install_text(&p, &p.hook_install(), None).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&config).unwrap()).unwrap();
        assert_eq!(v, serde_json::json!({"theme": "x", "plugin": ["other", path.display().to_string()]}));
        uninstall_provider(&p).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&config).unwrap()).unwrap();
        assert_eq!(v, serde_json::json!({"theme": "x", "plugin": ["other"]}));
        assert!(!path.exists());

        // A registration file am can't parse stops the install before anything is written.
        std::fs::write(&config, "{ // jsonc\n}").unwrap();
        assert!(install_text(&p, &p.hook_install(), None).is_err());
        assert!(!path.exists());
        assert!(check_provider(&p).is_err());
    }

    #[test]
    fn programs_are_read_from_generated_text() {
        let text = "a = \"\\\"/x y/am-hook\\\" hook-event --event s\"\nconst BIN = '/opt/am';\nb: \"/opt/custom\" hook-event\nc = \"/usr/bin/other\"\n";
        assert_eq!(hook_programs_in_text(text), [PathBuf::from("/x y/am-hook"), PathBuf::from("/opt/am"), PathBuf::from("/opt/custom")]);
        // An argv array (`HookContext::argv` as JSON) names any program; a
        // word that only starts with `hook-event` doesn't count.
        let argv = r#"const ARGV = ["/opt/my-hook", "hook-event", "--agent", "pi"]; const X = ["/usr/bin/env", "hook-events"];"#;
        assert_eq!(hook_programs_in_text(argv), [PathBuf::from("/opt/my-hook")]);
        // Spaces in the path, single quotes, the next element on its own line.
        let text = "a: [\"/opt/my hook\", \"hook-event\", \"--event\", \"x\"],\nb: [\"/usr/bin/other\", \"run\"],\nc: ['/opt/h2',\n  'hook-event'],\n";
        assert_eq!(hook_programs_in_text(text), [PathBuf::from("/opt/my hook"), PathBuf::from("/opt/h2")]);
        // The real thing: what `ctx.argv` renders through `js_string`.
        let ctx = HookContext {
            agent_id: "pi".into(),
            program: "/w x/am-hook-copy".into(),
            native: true,
            invocation: String::new(),
            hooks: Vec::new(),
        };
        let js = format!("const HOOK = [{}];", ctx.argv("x", "").iter().map(|a| js_string(a)).collect::<Vec<_>>().join(", "));
        assert_eq!(hook_programs_in_text(&js), [PathBuf::from("/w x/am-hook-copy")]);
    }

    /// The way a built-in harness branch declares a text-block style.
    static TOMLCLI: crate::harness::embedded::Embedded = crate::harness::embedded::Embedded::new(
        || {
            crate::harness::manifest::Manifest::builtin(
                r#"{"id":"tomlcli","name":"TOML CLI","program":"tomlcli",
                    "hooks":{"settingsPath":"~/.tomlcli/config.toml","timeoutMs":30000},
                    "events":{"Stop":"Stop"}}"#,
            )
        },
        None,
    )
    .with_hook_install(|| HookInstall::TextBlock(TextBlock { comment: "#", render: render_toml }));

    #[test]
    fn an_embedded_harness_picks_its_style() {
        let ctx = HookContext::new(&TOMLCLI, Some(Path::new("/b/am-hook")));
        assert!(matches!(TOMLCLI.hook_install(), HookInstall::TextBlock(_)));
        assert_eq!(
            ctx.hooks,
            [HookCommand {
                event: "Stop".into(),
                command: "\"/b/am-hook\" hook-event --agent tomlcli --worktree \"\" --event Stop".into(),
                timeout_ms: 30_000
            }]
        );
        assert_eq!(ctx.argv("Stop", "/w"), ["/b/am-hook", "hook-event", "--agent", "tomlcli", "--worktree", "/w", "--event", "Stop"]);
    }

    #[test]
    fn json_is_the_default_style() {
        assert!(matches!(crate::harness::HarnessKind::GEMINI.provider().hook_install(), HookInstall::Json));
        assert!(matches!(crate::harness::HarnessKind::CLAUDE.provider().hook_install(), HookInstall::Json));
    }
}
