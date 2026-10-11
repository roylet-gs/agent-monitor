//! `am-rs` with no subcommand: hand the terminal to the `am-tui` binary.

use std::os::unix::process::CommandExt;
use std::process::Command;

use am_core::debug;

pub fn launch(watch: bool, setup: bool) {
    let Some(bin) = super::find_sibling_or_path("am-tui") else {
        crate::fail("am-tui not found next to am-rs or on PATH.\nInstall it with: cargo install --path crates/am-tui");
    };
    // am-tui has no log pane or setup wizard yet; the flags are accepted so
    // scripts written for the TS `am` keep working.
    debug!("cli", "Launching {} (watch={watch}, setup={setup})", bin.display());
    let err = Command::new(&bin).exec();
    crate::fail(&format!("Failed to launch {}: {err}", bin.display()));
}
