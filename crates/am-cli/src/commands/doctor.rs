//! `am doctor` (src/commands/doctor.ts), plus a check for the native hook binary.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use am_core::{paths, settings};
use serde_json::{Value, json};

use crate::hooks_installer as hi;
use crate::output;

struct Check {
    name: String,
    status: &'static str, // ok | warn | fail
    message: String,
}

/// `gh --version` within 3 s (the TS `isGhAvailable`).
fn gh_available() -> bool {
    let Ok(mut child) = Command::new("gh").arg("--version").stdout(Stdio::null()).stderr(Stdio::null()).spawn() else {
        return false;
    };
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match child.try_wait() {
            Ok(Some(s)) => return s.success(),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

pub fn run(as_json: bool) {
    let mut checks = Vec::new();
    let app_dir = paths::app_dir();
    let app_exists = app_dir.exists();
    checks.push(Check {
        name: "App directory".into(),
        status: if app_exists { "ok" } else { "warn" },
        message: if app_exists {
            app_dir.display().to_string()
        } else {
            format!("Missing: {} (will be created on first use)", app_dir.display())
        },
    });

    let db = am_core::db::open().and_then(|c| Ok((am_core::db::repositories(&c)?, am_core::db::worktrees(&c)?)));
    checks.push(match db {
        Ok((repos, wts)) => Check {
            name: "Database".into(),
            status: "ok",
            message: format!("{} — {} repos, {} worktrees", paths::db_path().display(), repos.len(), wts.len()),
        },
        Err(e) => Check { name: "Database".into(), status: "fail", message: format!("Failed to open: {e}") },
    });

    let settings_path = paths::settings_path();
    let has_settings = settings_path.exists();
    checks.push(Check {
        name: "Settings".into(),
        status: if has_settings { "ok" } else { "warn" },
        message: if has_settings { settings_path.display().to_string() } else { "Using defaults (no settings.json)".into() },
    });

    let s = settings::load();
    for &kind in am_core::agents::all() {
        let p = kind.provider();
        let hooks = hi::is_hooks_installed(kind);
        let enabled = am_core::agents::is_enabled(kind, &s);
        // Claude is always checked (1.x output); other agents once they are in use.
        if kind != am_core::agents::AgentKind::CLAUDE && !hooks && !(enabled && p.is_present()) {
            continue;
        }
        let path = hi::display_path(p);
        let note = p.notes().map(|n| format!(" ({n})")).unwrap_or_default();
        let dangling = if hooks { hi::dangling_hook_binaries(kind) } else { Vec::new() };
        let (status, message) = match hi::read_settings(&p.settings_path()) {
            Err(e) => ("warn", e.to_string()),
            Ok(_) if !dangling.is_empty() => (
                "warn",
                format!(
                    "Hooks in {path} run a missing {}. Run: am hooks install --agent {}",
                    dangling.iter().map(|b| b.display().to_string()).collect::<Vec<_>>().join(", "),
                    kind.id()
                ),
            ),
            Ok(_) if !enabled && hooks => ("warn", format!("Plugin disabled but hooks remain in {path}. Run: am hooks uninstall --agent {}", kind.id())),
            Ok(_) if !enabled => ("ok", "Plugin disabled".into()),
            Ok(_) if !hooks => ("warn", format!("Not installed. Run: am hooks install --agent {}", kind.id())),
            Ok(_) => ("ok", format!("Installed in {path}{note}")),
        };
        checks.push(Check { name: format!("{} hooks", p.short_name()), status, message });
    }
    let plugin_errors = am_core::agents::plugin_errors();
    if !plugin_errors.is_empty() {
        checks.push(Check {
            name: "Agent plugins".into(),
            status: "warn",
            message: format!("Skipped in {}: {}", am_core::agents::plugins_dir().display(), plugin_errors.join("; ")),
        });
    }

    let native = hi::find_native_hook_binary();
    checks.push(Check {
        name: "Native hook".into(),
        status: if native.is_some() { "ok" } else { "warn" },
        message: match native {
            Some(p) => format!("{} (re-run `am hooks install` to point hooks at it)", p.display()),
            None => "am-hook not found; hooks fall back to `am hook-event`".into(),
        },
    });

    let gh = gh_available();
    checks.push(Check {
        name: "gh CLI".into(),
        status: if gh || !s.gh_pr_status { "ok" } else { "warn" },
        message: if gh { "Available".into() } else { "Not found (PR status disabled)".into() },
    });

    let ttyd = super::find_sibling_or_path("ttyd").is_some();
    checks.push(Check {
        name: "ttyd".into(),
        status: if ttyd { "ok" } else { "warn" },
        message: if ttyd { "Available (TUI screenshots enabled)".into() } else { "Not found. Install with: brew install ttyd".into() },
    });

    // Theme files that don't parse fall back to ansi in the TUI; surface them.
    let custom = am_core::themes::custom_names();
    let broken: Vec<&String> = custom.iter().filter(|n| am_core::themes::load(n).is_err()).collect();
    let theme_ok = am_core::themes::exists(&s.theme);
    checks.push(Check {
        name: "Theme".into(),
        status: if theme_ok && broken.is_empty() { "ok" } else { "warn" },
        message: if !theme_ok {
            format!("Unknown theme '{}' (the TUI uses ansi). Run: am theme list", s.theme)
        } else if !broken.is_empty() {
            format!("{} — unreadable files in {}: {}", s.theme, paths::themes_dir().display(), broken.iter().map(|n| n.as_str()).collect::<Vec<_>>().join(", "))
        } else {
            format!("{} ({} custom in {})", s.theme, custom.len(), paths::themes_dir().display())
        },
    });

    if s.linear_enabled {
        let has_key = !s.linear_api_key.is_empty();
        checks.push(Check {
            name: "Linear".into(),
            status: if has_key { "ok" } else { "warn" },
            message: if has_key { "Enabled with API key".into() } else { "Enabled but no API key set".into() },
        });
    }

    if as_json {
        let rows: Vec<Value> =
            checks.iter().map(|c| json!({ "name": c.name, "status": c.status, "message": c.message })).collect();
        output::json(&Value::Array(rows));
        return;
    }
    for c in &checks {
        let icon = match c.status {
            "ok" => "OK",
            "warn" => "WARN",
            _ => "FAIL",
        };
        println!("{:<7} {}: {}", format!("[{icon}]"), c.name, c.message);
    }
}
