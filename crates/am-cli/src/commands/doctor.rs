//! `am doctor` (src/commands/doctor.ts), plus a check for the native hook binary.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use am_core::{paths, settings};
use serde_json::{Value, json};

use crate::hooks_installer as hi;
use crate::output;

struct Check {
    name: &'static str,
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
        name: "App directory",
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
            name: "Database",
            status: "ok",
            message: format!("{} — {} repos, {} worktrees", paths::db_path().display(), repos.len(), wts.len()),
        },
        Err(e) => Check { name: "Database", status: "fail", message: format!("Failed to open: {e}") },
    });

    let settings_path = paths::settings_path();
    let has_settings = settings_path.exists();
    checks.push(Check {
        name: "Settings",
        status: if has_settings { "ok" } else { "warn" },
        message: if has_settings { settings_path.display().to_string() } else { "Using defaults (no settings.json)".into() },
    });

    let hooks = hi::is_global_hooks_installed();
    checks.push(Check {
        name: "Claude hooks",
        status: if hooks { "ok" } else { "warn" },
        message: if hooks { "Installed in ~/.claude/settings.json".into() } else { "Not installed. Run: am hooks install".into() },
    });

    let native = hi::find_native_hook_binary();
    checks.push(Check {
        name: "Native hook",
        status: if native.is_some() { "ok" } else { "warn" },
        message: match native {
            Some(p) => format!("{} (re-run `am hooks install` to point hooks at it)", p.display()),
            None => "am-hook not found; hooks fall back to `am hook-event`".into(),
        },
    });

    let s = settings::load();
    let gh = gh_available();
    checks.push(Check {
        name: "gh CLI",
        status: if gh || !s.gh_pr_status { "ok" } else { "warn" },
        message: if gh { "Available".into() } else { "Not found (PR status disabled)".into() },
    });

    let ttyd = super::find_sibling_or_path("ttyd").is_some();
    checks.push(Check {
        name: "ttyd",
        status: if ttyd { "ok" } else { "warn" },
        message: if ttyd { "Available (TUI screenshots enabled)".into() } else { "Not found. Install with: brew install ttyd".into() },
    });

    // Theme files that don't parse fall back to ansi in the TUI; surface them.
    let custom = am_core::themes::custom_names();
    let broken: Vec<&String> = custom.iter().filter(|n| am_core::themes::load(n).is_err()).collect();
    let theme_ok = am_core::themes::exists(&s.theme);
    checks.push(Check {
        name: "Theme",
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
            name: "Linear",
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
