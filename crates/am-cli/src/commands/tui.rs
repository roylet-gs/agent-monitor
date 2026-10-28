//! `am` with no subcommand: hand the terminal to the `am-tui` binary.

use std::os::unix::process::CommandExt;
use std::process::Command;

use am_core::debug;

/// What `am-tui` opens on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    /// Whatever `appMode` says.
    Dashboard,
    /// The setup wizard (`am setup`).
    Setup,
    /// The review page alone, this once (`am reviews`).
    Reviews,
}

pub fn launch(watch: bool, page: Page) {
    let Some(bin) = super::find_sibling_or_path("am-tui") else {
        crate::fail("am-tui not found next to am or on PATH.\nReinstall agent-monitor (npm i -g @roylet-gs/agent-monitor, or brew reinstall agent-monitor).");
    };
    debug!("cli", "Launching {} (watch={watch}, page={page:?})", bin.display());
    let mut cmd = Command::new(&bin);
    match page {
        Page::Dashboard => {}
        Page::Setup => {
            cmd.arg("--setup");
        }
        Page::Reviews => {
            cmd.arg("--reviews");
        }
    }
    if watch {
        cmd.arg("--log");
    }
    let err = cmd.exec();
    crate::fail(&format!("Failed to launch {}: {err}", bin.display()));
}
