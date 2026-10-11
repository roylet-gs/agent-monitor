//! Claude Code hook config in `~/.claude/settings.json`. Port of
//! `src/lib/hooks-installer.ts`: same events, same entry shape, same
//! "replace our entries, keep everyone else's" rules, so the TS and Rust
//! installers can run against each other's output.

use std::path::{Path, PathBuf};

use crate::{info, paths};
use serde_json::{Map, Value, json};

pub const HOOK_EVENTS: [&str; 11] = [
    "PreToolUse",
    "PostToolUse",
    "Stop",
    "StopFailure",
    "Notification",
    "SessionStart",
    "SessionEnd",
    "UserPromptSubmit",
    "SubagentStart",
    "SubagentStop",
    "PermissionRequest",
];

/// Events Claude Code rewrites to `--managed` with a 5-minute timeout; writing
/// them in that shape keeps install idempotent.
const MANAGED_HOOK_EVENTS: [&str; 1] = ["PermissionRequest"];

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

/// Appended to native hook commands; see `build_hook_entry`.
pub const NATIVE_COMPAT_SUFFIX: &str = " # am hook-event (native)";

pub fn build_hook_entry(event: &str, native_bin: Option<&Path>) -> Value {
    let managed = MANAGED_HOOK_EVENTS.contains(&event);
    let managed_flag = if managed { " --managed" } else { "" };
    // The native binary reads stdin itself, so it doesn't need the `cat |` shim.
    let invocation = match native_bin {
        Some(bin) => format!("\"{}\" hook-event", bin.display()),
        None => "cat | am hook-event".to_string(),
    };
    // Old TS releases (≤1.0.95) only recognise their hooks by the literal
    // text `am hook-event`; without it they think hooks are missing and add a
    // second, TS handler that races this one and overwrites `delegating`.
    // A trailing shell comment keeps them seeing the native hook as "ours".
    let compat = if native_bin.is_some() { NATIVE_COMPAT_SUFFIX } else { "" };
    json!({
        "hooks": [{
            "type": "command",
            "command": format!("{invocation} --worktree \"$CLAUDE_PROJECT_DIR\" --event {event}{managed_flag}{compat}"),
            "timeout": if managed { 300_000 } else { 5_000 },
        }]
    })
}

pub fn global_settings_path() -> PathBuf {
    paths::home_dir().join(".claude").join("settings.json")
}

pub fn read_global_settings() -> Map<String, Value> {
    std::fs::read_to_string(global_settings_path())
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| match v {
            Value::Object(m) => Some(m),
            _ => None,
        })
        .unwrap_or_default()
}

pub fn write_global_settings(settings: &Map<String, Value>) -> std::io::Result<()> {
    let path = global_settings_path();
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

/// Returns the binary the hooks now point at (`None` = the TS `am hook-event`).
pub fn install_global_hooks() -> std::io::Result<Option<PathBuf>> {
    let mut settings = read_global_settings();
    let mut hooks = hooks_of(&settings);
    let native = find_native_hook_binary();
    // Replace (rather than skip) existing am entries so re-running install
    // switches between the TS command and the native binary.
    for event in HOOK_EVENTS {
        let existing = hooks.get(event).and_then(Value::as_array).cloned().unwrap_or_default();
        let mut next = without_am_hooks(&existing);
        next.push(build_hook_entry(event, native.as_deref()));
        hooks.insert(event.to_string(), Value::Array(next));
    }
    settings.insert("hooks".into(), Value::Object(hooks));
    write_global_settings(&settings)?;
    info!(
        "hooks",
        "Installed global hooks into {} using {}",
        global_settings_path().display(),
        native.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "am hook-event (TS)".into())
    );
    Ok(native)
}

pub fn uninstall_global_hooks() -> std::io::Result<()> {
    let mut settings = read_global_settings();
    let mut hooks = hooks_of(&settings);
    for event in HOOK_EVENTS {
        let Some(existing) = hooks.get(event).and_then(Value::as_array).cloned() else { continue };
        let kept = without_am_hooks(&existing);
        if kept.is_empty() {
            hooks.shift_remove(event);
        } else {
            hooks.insert(event.to_string(), Value::Array(kept));
        }
    }
    if hooks.is_empty() {
        settings.shift_remove("hooks");
    } else {
        settings.insert("hooks".into(), Value::Object(hooks));
    }
    write_global_settings(&settings)?;
    info!("hooks", "Uninstalled global hooks from {}", global_settings_path().display());
    Ok(())
}

pub fn is_global_hooks_installed() -> bool {
    let hooks = hooks_of(&read_global_settings());
    HOOK_EVENTS.iter().any(|e| hooks.get(*e).and_then(Value::as_array).is_some_and(|ms| ms.iter().any(matcher_is_ours)))
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
    fn entry_shape_matches_ts() {
        let e = build_hook_entry("PermissionRequest", Some(Path::new("/bin/am-hook")));
        assert_eq!(
            e["hooks"][0]["command"],
            "\"/bin/am-hook\" hook-event --worktree \"$CLAUDE_PROJECT_DIR\" --event PermissionRequest --managed # am hook-event (native)"
        );
        assert_eq!(e["hooks"][0]["timeout"], 300_000);
        let e = build_hook_entry("Stop", None);
        assert_eq!(e["hooks"][0]["command"], "cat | am hook-event --worktree \"$CLAUDE_PROJECT_DIR\" --event Stop");
        assert_eq!(e["hooks"][0]["timeout"], 5_000);
    }
}
