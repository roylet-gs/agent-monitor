//! Hook config in each agent CLI's settings file (`~/.claude/settings.json`,
//! `~/.gemini/settings.json`). The entry shapes come from the providers in
//! `crate::harness`; the rules are shared and ported from the TS
//! `src/lib/hooks-installer.ts`: replace our entries, keep everyone else's, so
//! the TS and Rust installers can run against each other's output.

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

fn is_executable(path: &Path) -> bool {
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

/// The native `am-hook` binary to point hooks at: `$AM_HOOK_BIN`, then the
/// one next to this executable (a `cargo install`/release puts them side by
/// side), then `$PATH`, then `~/.cargo/bin`. `$AM_NO_NATIVE_HOOK` opts out.
///
/// Hook commands outlive the binary that wrote them, so two cases prefer a
/// stable path: a sibling that `$PATH` reaches through a symlink (Homebrew's
/// `bin/am-hook` → the versioned `Cellar/…` path `current_exe` resolves to,
/// which `brew upgrade` removes) is written as the `$PATH` entry, and a
/// sibling in a cargo `target/` dir (a dev build) is used only when no other
/// `am-hook` is installed.
pub fn find_native_hook_binary() -> Option<PathBuf> {
    if std::env::var_os("AM_NO_NATIVE_HOOK").is_some_and(|v| !v.is_empty()) {
        return None;
    }
    if let Some(p) = std::env::var_os("AM_HOOK_BIN").filter(|p| !p.is_empty()).map(PathBuf::from)
        && is_executable(&p)
    {
        return Some(p);
    }
    let sibling = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join("am-hook")))
        .filter(|p| is_executable(p));
    let mut installed: Vec<PathBuf> = Vec::new();
    if let Some(path) = std::env::var_os("PATH") {
        installed.extend(std::env::split_paths(&path).filter(|d| !d.as_os_str().is_empty()).map(|d| d.join("am-hook")));
    }
    installed.push(paths::home_dir().join(".cargo").join("bin").join("am-hook"));
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
    dangling_in(&read_settings(&agent.provider().settings_path()).unwrap_or_default())
}

fn dangling_in(settings: &Map<String, Value>) -> Vec<PathBuf> {
    let mut missing: Vec<PathBuf> = hooks_of(settings)
        .values()
        .filter_map(Value::as_array)
        .flatten()
        .filter_map(|m| m.get("hooks").and_then(Value::as_array))
        .flatten()
        .filter_map(|h| h.get("command").and_then(Value::as_str))
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
    entry_for(agent.provider(), event, native_bin)
}

fn entry_for(provider: &dyn Harness, event: &str, native_bin: Option<&Path>) -> Value {
    provider.hook_entry(event, &invocation(native_bin), native_bin.is_some())
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
pub fn install_hooks(agent: HarnessKind) -> std::io::Result<Option<PathBuf>> {
    install_for(agent.provider())
}

/// [`install_hooks`] for any provider, registered or not (a manifest being
/// added or published by `harness::lifecycle`).
pub fn install_for(provider: &dyn Harness) -> std::io::Result<Option<PathBuf>> {
    let path = provider.settings_path();
    let mut settings = read_settings(&path)?;
    let mut hooks = hooks_of(&settings);
    let native = find_native_hook_binary();
    // Replace (rather than skip) existing am entries so re-running install
    // switches between the TS command and the native binary.
    for event in provider.hook_events() {
        let existing = hooks.get(event.as_str()).and_then(Value::as_array).cloned().unwrap_or_default();
        let mut next = without_am_hooks(&existing);
        next.push(entry_for(provider, &event, native.as_deref()));
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

pub fn uninstall_hooks(agent: HarnessKind) -> std::io::Result<()> {
    uninstall_for(agent.provider())
}

/// [`uninstall_hooks`] for any provider, registered or not.
pub fn uninstall_for(provider: &dyn Harness) -> std::io::Result<()> {
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

pub fn is_hooks_installed(agent: HarnessKind) -> bool {
    is_installed_for(agent.provider())
}

pub fn is_installed_for(provider: &dyn Harness) -> bool {
    let hooks = hooks_of(&read_settings(&provider.settings_path()).unwrap_or_default());
    provider.hook_events().iter().any(|e| hooks.get(e.as_str()).and_then(Value::as_array).is_some_and(|ms| ms.iter().any(matcher_is_ours)))
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
        assert_eq!(dangling_in(&settings), [PathBuf::from("/gone/am-hook")]);
        assert!(dangling_in(&Map::new()).is_empty());
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
