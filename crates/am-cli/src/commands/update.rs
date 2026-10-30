//! `am update`: install the latest release the way this copy was installed
//! (`am_core::update_check`; the TUI's update notice runs the same steps).

use am_core::update_check::{self, InstallMethod};
use serde_json::json;

use crate::output;

/// `check_only` (`--check`) and `as_json` (`--json`, which implies it) only
/// report; otherwise a newer release is installed.
pub fn run(check_only: bool, as_json: bool) {
    let current = env!("CARGO_PKG_VERSION");
    let method = InstallMethod::current();
    let latest = update_check::latest_now().unwrap_or_else(|e| crate::fail(&format!("Couldn't check for a newer release: {e}")));
    let newer = update_check::is_newer(&latest, current);
    let command = method.upgrade_steps().map(|s| update_check::shell_text(&s));
    if as_json {
        output::json(&json!({
            "current": current,
            "latest": latest,
            "updateAvailable": newer,
            "installMethod": method.id(),
            "command": command,
            "hint": method.upgrade_hint(),
        }));
        return;
    }
    if !newer {
        println!("am v{current} is up to date");
        return;
    }
    if check_only {
        println!("am v{latest} is available (you have v{current}). Update: {}", command.unwrap_or_else(|| method.upgrade_hint()));
        return;
    }
    let Some(command) = command else {
        crate::fail(&format!("am v{latest} is available (you have v{current}), but am can't update this install itself: {}", method.upgrade_hint()));
    };
    eprintln!("Updating am v{current} → v{latest}: {command}");
    match update_check::run_upgrade(method, |line| println!("{line}")) {
        Ok(()) => eprintln!("✓ Updated to am v{latest}. Restart any open am dashboards to use it."),
        Err(e) => crate::fail(&format!("Update failed: {e}")),
    }
}
