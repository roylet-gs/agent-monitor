//! `am cmux dock`: put am in cmux's Dock (its right sidebar), as a control in
//! `~/.config/cmux/dock.json` or the project's `.cmux/dock.json`.

use std::path::Path;

use am_core::cmux::{self, am_dock_controls, merge_dock};
use am_core::hooks_installer::{read_settings, tilde, write_settings};
use am_core::info;

use crate::fail;

pub fn dock(project: Option<&Path>, reviews: bool, remove: bool) {
    let path = cmux::dock_path(project);
    let existed = path.exists();
    if remove && !existed {
        println!("Nothing to remove: {} doesn't exist", tilde(&path));
        return;
    }
    let mut dock = read_settings(&path).unwrap_or_else(|e| fail(&format!("Couldn't read {}: {e}", tilde(&path))));
    // A project's dock.json is committed and shared, so it names plain `am`
    // (the Dock runs commands in a login shell); the personal one names this
    // machine's binary.
    let am = match project {
        Some(_) => "am".to_string(),
        None => am_core::hooks_installer::find_installed_binary("am")
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "am".into()),
    };
    // Removing takes out every control am adds, whatever was asked for then.
    let controls = am_dock_controls(&am, reviews || remove);
    if remove && !cmux::has_am_controls(&dock, &controls) {
        println!("Nothing to remove: {} has no am controls", tilde(&path));
        return;
    }
    if let Err(e) = merge_dock(&mut dock, &controls, remove) {
        fail(&format!("{} isn't a Dock config: {e}", tilde(&path)));
    }
    // The project's file is in git, which keeps its history: no backup
    // littering `.cmux/`.
    if existed && project.is_none() {
        let stamp = time::OffsetDateTime::now_utc().unix_timestamp();
        let backup = path.with_file_name(format!("dock.json.{stamp}.bak"));
        if let Err(e) = std::fs::copy(&path, &backup) {
            fail(&format!("Couldn't back up {}: {e}", tilde(&path)));
        }
        println!("Backed up the old file to {}", tilde(&backup));
    }
    if let Err(e) = write_settings(&path, &dock) {
        fail(&format!("Couldn't write {}: {e}", tilde(&path)));
    }
    if remove {
        info!("cmux", "Removed am from the cmux Dock in {}", path.display());
        println!("✓ Removed am from {}", tilde(&path));
        return;
    }
    info!("cmux", "Added am to the cmux Dock in {}", path.display());
    println!("✓ Added {} to {}", controls.iter().map(|c| c.title).collect::<Vec<_>>().join(" and "), tilde(&path));
    println!("  A Dock that cmux already saved isn't seeded again: reload it from the Dock's menu (or add the control there).");
    if project.is_some() {
        println!("  cmux asks you to trust a project's Dock config the first time it runs it.");
    }
    println!("  The Dock is narrow: Settings → Appearance → Compact view (or `am settings set compactView true`) fits it best.");
}
