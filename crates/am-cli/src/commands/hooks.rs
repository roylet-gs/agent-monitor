//! `am hooks install|uninstall|status` (src/commands/hooks.ts).

use crate::hooks_installer as hi;
use crate::{fail, output};

pub fn install() {
    match hi::install_global_hooks() {
        Ok(_) => println!("Hooks installed into ~/.claude/settings.json"),
        Err(e) => fail(&format!("Failed to install hooks: {e}")),
    }
}

pub fn uninstall() {
    match hi::uninstall_global_hooks() {
        Ok(()) => println!("Hooks removed from ~/.claude/settings.json"),
        Err(e) => fail(&format!("Failed to remove hooks: {e}")),
    }
}

pub fn status(json: bool) {
    let installed = hi::is_global_hooks_installed();
    if json {
        output::json(&serde_json::json!({ "installed": installed }));
        return;
    }
    println!("Claude hooks: {}", if installed { "installed" } else { "not installed" });
}
