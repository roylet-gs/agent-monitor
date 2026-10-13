//! Hook config in each agent CLI's settings file (`~/.claude/settings.json`,
//! `~/.gemini/settings.json`). The entry shapes come from the providers in
//! `crate::agents`; the rules are shared and ported from the TS
//! `src/lib/hooks-installer.ts`: replace our entries, keep everyone else's, so
//! the TS and Rust installers can run against each other's output.

use std::path::{Path, PathBuf};

use crate::agents::{AgentKind, AgentProvider};
use crate::{info, paths};
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

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// The native `am-hook` binary to point hooks at: `$AM_HOOK_BIN`, then the
/// one next to this executable (a `cargo install`/release puts them side by
/// side), then `$PATH`, then `~/.cargo/bin`. `$AM_NO_NATIVE_HOOK` opts out.
pub fn find_native_hook_binary() -> Option<PathBuf> {
    if std::env::var_os("AM_NO_NATIVE_HOOK").is_some_and(|v| !v.is_empty()) {
        return None;
    }
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(p) = std::env::var_os("AM_HOOK_BIN").filter(|p| !p.is_empty()) {
        candidates.push(PathBuf::from(p));
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        candidates.push(dir.join("am-hook"));
    }
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path).filter(|d| !d.as_os_str().is_empty()).map(|d| d.join("am-hook")));
    }
    candidates.push(paths::home_dir().join(".cargo").join("bin").join("am-hook"));
    candidates.into_iter().find(|c| is_executable(c))
}

/// The program part of a hook command: the native binary when there is one,
/// else the `am` CLI (`cat |` shim kept from the TS app).
fn invocation(native_bin: Option<&Path>) -> String {
    match native_bin {
        Some(bin) => format!("\"{}\" hook-event", bin.display()),
        None => "cat | am hook-event".to_string(),
    }
}

pub fn build_hook_entry(agent: AgentKind, event: &str, native_bin: Option<&Path>) -> Value {
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

fn matcher_is_ours(m: &Value) -> bool {
    m.get("hooks")
        .and_then(Value::as_array)
        .is_some_and(|hs| hs.iter().any(|h| h.get("command").and_then(Value::as_str).is_some_and(is_am_hook_command)))
}

fn without_am_hooks(matchers: &[Value]) -> Vec<Value> {
    matchers.iter().filter(|m| !matcher_is_ours(m)).cloned().collect()
}

fn hooks_of(settings: &Map<String, Value>) -> Map<String, Value> {
    match settings.get("hooks") {
        Some(Value::Object(h)) => h.clone(),
        _ => Map::new(),
    }
}

/// Install `agent`'s hooks. Returns the binary they now point at (`None` =
/// the `am hook-event` CLI fallback).
pub fn install_hooks(agent: AgentKind) -> std::io::Result<Option<PathBuf>> {
    let provider = agent.provider();
    let path = provider.settings_path();
    let mut settings = read_settings(&path)?;
    let mut hooks = hooks_of(&settings);
    let native = find_native_hook_binary();
    // Replace (rather than skip) existing am entries so re-running install
    // switches between the TS command and the native binary.
    for event in provider.hook_events() {
        let existing = hooks.get(event.as_str()).and_then(Value::as_array).cloned().unwrap_or_default();
        let mut next = without_am_hooks(&existing);
        next.push(build_hook_entry(agent, &event, native.as_deref()));
        hooks.insert(event, Value::Array(next));
    }
    settings.insert("hooks".into(), Value::Object(hooks));
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

pub fn uninstall_hooks(agent: AgentKind) -> std::io::Result<()> {
    let provider = agent.provider();
    let path = provider.settings_path();
    if !path.exists() {
        return Ok(());
    }
    let mut settings = read_settings(&path)?;
    let mut hooks = hooks_of(&settings);
    for event in provider.hook_events() {
        let Some(existing) = hooks.get(event.as_str()).and_then(Value::as_array).cloned() else { continue };
        let kept = without_am_hooks(&existing);
        if kept.is_empty() {
            hooks.shift_remove(event.as_str());
        } else {
            hooks.insert(event.to_string(), Value::Array(kept));
        }
    }
    if hooks.is_empty() {
        settings.shift_remove("hooks");
    } else {
        settings.insert("hooks".into(), Value::Object(hooks));
    }
    write_settings(&path, &settings)?;
    info!("hooks", "Uninstalled {} hooks from {}", provider.name(), path.display());
    Ok(())
}

pub fn is_hooks_installed(agent: AgentKind) -> bool {
    let provider = agent.provider();
    let hooks = hooks_of(&read_settings(&provider.settings_path()).unwrap_or_default());
    provider.hook_events().iter().any(|e| hooks.get(e.as_str()).and_then(Value::as_array).is_some_and(|ms| ms.iter().any(matcher_is_ours)))
}

/// Display form of a provider's settings path (`~/.gemini/settings.json`).
pub fn display_path(p: &dyn AgentProvider) -> String {
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
}
