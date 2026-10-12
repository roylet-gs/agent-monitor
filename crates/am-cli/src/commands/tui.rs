//! `am` with no subcommand: hand the terminal to the `am-tui` binary.

use std::os::unix::process::CommandExt;
use std::process::Command;

use am_core::debug;

pub fn launch(watch: bool, setup: bool, sidebar: bool) {
    let Some(bin) = super::find_sibling_or_path("am-tui") else {
        crate::fail("am-tui not found next to am or on PATH.\nReinstall agent-monitor (npm i -g @roylet-gs/agent-monitor, or brew reinstall agent-monitor).");
    };
    debug!("cli", "Launching {} (watch={watch}, setup={setup}, sidebar={sidebar})", bin.display());
    let mut cmd = Command::new(&bin);
    if setup {
        cmd.arg("--setup");
    }
    if watch {
        cmd.arg("--log");
    }
    if sidebar {
        cmd.arg("--sidebar");
    }
    let err = cmd.exec();
    crate::fail(&format!("Failed to launch {}: {err}", bin.display()));
}
