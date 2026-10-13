//! `am hooks install|uninstall|status` (src/commands/hooks.ts), per agent
//! plugin (`am_core::harness`).

use am_core::harness::{self, HarnessKind};
use am_core::settings;

use crate::hooks_installer as hi;
use crate::{fail, output};

/// `--agent` values, or `default` when none were given.
fn selected(ids: &[String], default: Vec<HarnessKind>) -> Vec<HarnessKind> {
    if ids.is_empty() {
        return default;
    }
    ids.iter()
        .map(|id| {
            HarnessKind::get(id).unwrap_or_else(|| fail(&format!("Unknown agent: {id}. Valid agents: {}", harness::ids().join(", "))))
        })
        .collect()
}

/// Default: Claude plus every other enabled agent CLI found on this machine.
pub fn install(ids: &[String]) {
    let s = settings::load();
    let mut failed = false;
    for kind in selected(ids, harness::detected(&s)) {
        let p = kind.provider();
        if !harness::is_enabled(kind, &s) {
            eprintln!("{} is disabled (disabledAgents); enable it in settings first", p.name());
            failed = true;
            continue;
        }
        match hi::install_hooks(kind) {
            Ok(_) => println!("Hooks installed into {}", hi::display_path(p)),
            Err(e) => {
                eprintln!("Failed to install {} hooks: {e}", p.name());
                failed = true;
            }
        }
    }
    if failed {
        std::process::exit(1);
    }
}

/// Default: every agent that has our hooks (Claude always, as in 1.x).
pub fn uninstall(ids: &[String]) {
    let explicit = !ids.is_empty();
    let mut failed = false;
    for kind in selected(ids, harness::all().to_vec()) {
        let p = kind.provider();
        // Leave files without our hooks alone (they may not even be parseable).
        if !explicit && kind != HarnessKind::CLAUDE && !hi::is_hooks_installed(kind) {
            continue;
        }
        match hi::uninstall_hooks(kind) {
            Ok(()) => println!("Hooks removed from {}", hi::display_path(p)),
            Err(e) => {
                eprintln!("Failed to remove {} hooks: {e}", p.name());
                failed = true;
            }
        }
    }
    if failed {
        std::process::exit(1);
    }
}

pub fn status(json: bool) {
    let states: Vec<(HarnessKind, bool)> = harness::all().iter().map(|k| (*k, hi::is_hooks_installed(*k))).collect();
    if json {
        let per_agent: serde_json::Map<String, serde_json::Value> =
            states.iter().map(|(k, on)| (k.id().to_string(), (*on).into())).collect();
        // `installed` (Claude) is the 1.x shape; `agents` adds the rest.
        output::json(&serde_json::json!({ "installed": states[0].1, "agents": per_agent }));
        return;
    }
    for (kind, on) in states {
        // Claude is always listed (1.x output); others once present or installed.
        if kind != HarnessKind::CLAUDE && !on && !kind.provider().is_present() {
            continue;
        }
        println!("{} hooks: {}", kind.provider().short_name(), if on { "installed" } else { "not installed" });
    }
}
