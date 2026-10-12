//! `am sidebar`: toggle a narrow `am-tui --sidebar` split next to your work.
//!
//! tmux: a full-height pane tagged with the `@am-sidebar` pane option, so a
//! second call finds and closes it. Ghostty (macOS): an AppleScript split;
//! `am-tui --sidebar` titles its terminal `am sidebar`, which is how the next
//! call finds it. Anything else can't be split from outside.

use std::process::{Command, Stdio};

use am_core::terminal::{self, Backend, applescript_string, sh_quote};
use am_core::{info, settings};

use crate::fail;

pub const SIDEBAR_TITLE: &str = "am sidebar";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

impl Side {
    pub fn parse(s: &str) -> Option<Side> {
        match s {
            "left" => Some(Side::Left),
            "right" => Some(Side::Right),
            _ => None,
        }
    }
}

fn tmux(args: &[&str]) -> Result<String, String> {
    let out = Command::new("tmux").args(args).stdin(Stdio::null()).output().map_err(|e| format!("tmux: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// The tagged sidebar pane among `id<TAB>@am-sidebar` lines.
pub fn find_tagged_pane(list: &str) -> Option<&str> {
    list.lines().find_map(|l| l.split_once('\t').filter(|(_, tag)| tag.trim() == "1").map(|(id, _)| id))
}

/// `split-window` argv (after `tmux`) for a new sidebar.
pub fn tmux_split_args(tui: &str, side: Side, width: u16, target: Option<&str>) -> Vec<String> {
    let mut a: Vec<String> = vec!["split-window".into(), "-h".into(), "-f".into()];
    if side == Side::Left {
        a.push("-b".into());
    }
    if let Some(t) = target {
        a.extend(["-t".into(), t.into()]);
    }
    a.extend(["-l".into(), width.to_string(), "-P".into(), "-F".into(), "#{pane_id}".into()]);
    a.push(format!("{} --sidebar", sh_quote(tui)));
    a
}

fn toggle_tmux(tui: &str, side: Side, width: u16, target: Option<&str>) -> Result<&'static str, String> {
    let mut list = vec!["list-panes", "-F", "#{pane_id}\t#{@am-sidebar}"];
    if let Some(t) = target {
        list.extend(["-t", t]);
    }
    if let Some(id) = find_tagged_pane(&tmux(&list)?) {
        tmux(&["kill-pane", "-t", id])?;
        return Ok("closed");
    }
    let args = tmux_split_args(tui, side, width, target);
    let id = tmux(&args.iter().map(String::as_str).collect::<Vec<_>>())?;
    tmux(&["set-option", "-p", "-t", id.trim(), "@am-sidebar", "1"])?;
    Ok("opened")
}

pub fn ghostty_script(tui: &str, side: Side) -> String {
    let dir = if side == Side::Left { "left" } else { "right" };
    format!(
        "tell application \"Ghostty\"\n set tb to selected tab of front window\n repeat with t in terminals of tb\n  if name of t is {title} then\n   close t\n   return \"closed\"\n  end if\n end repeat\n set cfg to new surface configuration\n set command of cfg to {cmd}\n split (focused terminal of tb) direction {dir} with configuration cfg\n return \"opened\"\nend tell",
        title = applescript_string(SIDEBAR_TITLE),
        cmd = applescript_string(&format!("{} --sidebar", sh_quote(tui))),
    )
}

fn toggle_ghostty(tui: &str, side: Side) -> Result<&'static str, String> {
    let out = Command::new("osascript").arg("-e").arg(ghostty_script(tui, side)).stdin(Stdio::null()).output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(if String::from_utf8_lossy(&out.stdout).trim() == "closed" { "closed" } else { "opened" })
}

pub fn run(width: u16, position: &str, target: Option<&str>) {
    let side = Side::parse(position).unwrap_or_else(|| fail(&format!("Unknown position '{position}' (expected left or right)")));
    let Some(tui) = super::find_sibling_or_path("am-tui") else { fail("am-tui not found next to am or on PATH.") };
    let tui = tui.to_string_lossy().into_owned();
    let backend = terminal::detect(&settings::load().terminal_backend);
    let result = match backend {
        Backend::Tmux => toggle_tmux(&tui, side, width, target),
        Backend::Ghostty if cfg!(target_os = "macos") => toggle_ghostty(&tui, side),
        other => fail(&format!(
            "am sidebar needs tmux or Ghostty on macOS (detected: {}).\nRun `am --sidebar` in a narrow split instead.",
            other.name()
        )),
    };
    match result {
        Ok(what) => info!("cli", "Sidebar {what} ({})", backend.name()),
        Err(e) => fail(&format!("Couldn't toggle the sidebar in {}: {e}", backend.name())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_tagged_pane() {
        assert_eq!(find_tagged_pane("%1\t\n%4\t1\n"), Some("%4"));
        assert_eq!(find_tagged_pane("%1\t\n%2\t0\n"), None);
    }

    #[test]
    fn split_args() {
        assert_eq!(
            tmux_split_args("/bin/am-tui", Side::Left, 42, Some("%3")),
            ["split-window", "-h", "-f", "-b", "-t", "%3", "-l", "42", "-P", "-F", "#{pane_id}", "/bin/am-tui --sidebar"]
        );
        let right = tmux_split_args("/my bin/am-tui", Side::Right, 40, None);
        assert!(!right.contains(&"-b".to_string()));
        assert_eq!(right.last().unwrap(), "'/my bin/am-tui' --sidebar");
    }

    #[test]
    fn ghostty_toggle_script() {
        let s = ghostty_script("/bin/am-tui", Side::Left);
        assert!(s.contains("if name of t is \"am sidebar\" then"), "{s}");
        assert!(s.contains("set command of cfg to \"/bin/am-tui --sidebar\""), "{s}");
        assert!(s.contains("direction left"), "{s}");
    }
}
