//! `am integrate <tmux|ghostty>`: print, or install into the terminal's
//! config, the snippet that hooks am in. The installed block sits between
//! markers and is replaced in place on re-runs, so installing is idempotent.
//! TPM users get the same tmux setup from `agent-monitor.tmux`.

use std::path::{Path, PathBuf};

use am_core::terminal::sh_quote;
use am_core::{info, paths};

use crate::fail;

const BEGIN: &str = "# >>> agent-monitor >>>";
const END: &str = "# <<< agent-monitor <<<";

pub fn tmux_snippet(am: &str, tui: &str) -> String {
    let am = sh_quote(am);
    let tui = sh_quote(tui);
    format!(
        "{BEGIN}
# Managed by `am integrate tmux`: re-run it to update, or delete this block.
# Let am-tui's desktop notifications (OSC 9) reach the outer terminal.
set -g allow-passthrough on
# prefix + A: dashboard popup (closes after opening a worktree).
bind-key A display-popup -E -w 85% -h 85% -d '#{{pane_current_path}}' \"{tui} --popup\"
# prefix + S: toggle the dashboard as a sidebar.
bind-key S run-shell \"{am} sidebar --target '#{{pane_id}}'\"
# Agent counts in the status bar (once, even when this file is re-sourced).
if-shell -F '#{{m:*summary --format tmux*,#{{status-right}}}}' '' \"set -ag status-right ' #({am} summary --format tmux)'\"
{END}
"
    )
}

pub fn ghostty_snippet(am: &str) -> String {
    format!(
        "{BEGIN}
# Managed by `am integrate ghostty`: re-run it to update, or delete this block.
# am opens worktrees in tabs/splits of the front Ghostty window over AppleScript
# (Ghostty 1.3+, macOS asks once for Automation permission).
# Desktop notifications from am-tui (enable terminalNotifications in am settings).
desktop-notifications = true
# Optional: cmd+shift+a types `am sidebar` into the focused shell to toggle the
# dashboard split. Ghostty keybinds can't run commands, so it types the text;
# only use it if that pane is usually a shell prompt.
# keybind = super+shift+a=text:{am} sidebar\\r
{END}
"
    )
}

/// `existing` with the marked block replaced by `block` (or appended).
pub fn upsert_block(existing: &str, block: &str) -> String {
    if let (Some(start), Some(end_at)) = (existing.find(BEGIN), existing.find(END))
        && end_at > start
    {
        let mut end = end_at + END.len();
        if existing[end..].starts_with('\n') {
            end += 1;
        }
        return format!("{}{block}{}", &existing[..start], &existing[end..]);
    }
    let sep = if existing.is_empty() || existing.ends_with("\n\n") {
        ""
    } else if existing.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    format!("{existing}{sep}{block}")
}

fn tmux_conf(home: &Path) -> PathBuf {
    let xdg = home.join(".config/tmux/tmux.conf");
    if xdg.exists() { xdg } else { home.join(".tmux.conf") }
}

/// Ghostty reads `$XDG_CONFIG_HOME/ghostty/config` and, on macOS, the
/// Application Support file; prefer whichever exists (newer Ghostty also
/// names it `config.ghostty`).
fn ghostty_conf(home: &Path) -> PathBuf {
    let candidates = [
        home.join(".config/ghostty/config.ghostty"),
        home.join(".config/ghostty/config"),
        home.join("Library/Application Support/com.mitchellh.ghostty/config.ghostty"),
        home.join("Library/Application Support/com.mitchellh.ghostty/config"),
    ];
    candidates.iter().find(|p| p.exists()).cloned().unwrap_or_else(|| candidates[1].clone())
}

pub fn run(target: &str, install: bool) {
    let am = std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|_| "am".into());
    let home = paths::home_dir();
    let (snippet, conf, reload) = match target {
        "tmux" => {
            let tui = super::find_sibling_or_path("am-tui").map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|| "am-tui".into());
            let conf = tmux_conf(&home);
            let reload = format!("Reload with: tmux source-file {}", conf.display());
            (tmux_snippet(&am, &tui), conf, reload)
        }
        "ghostty" => (ghostty_snippet(&am), ghostty_conf(&home), "Reload Ghostty's config (cmd+shift+,) to apply.".to_string()),
        other => fail(&format!("Unknown terminal '{other}' (expected tmux or ghostty)")),
    };
    if !install {
        print!("{snippet}");
        return;
    }
    let existing = std::fs::read_to_string(&conf).unwrap_or_default();
    let updated = upsert_block(&existing, &snippet);
    if updated == existing {
        println!("{} is already up to date.", conf.display());
        return;
    }
    if let Some(dir) = conf.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = std::fs::write(&conf, updated) {
        fail(&format!("Couldn't write {}: {e}", conf.display()));
    }
    info!("cli", "Installed {target} integration into {}", conf.display());
    println!("✓ Installed the {target} integration into {}\n{reload}", conf.display());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_appends_then_replaces_in_place() {
        let block = format!("{BEGIN}\nv1\n{END}\n");
        let once = upsert_block("set -g mouse on\n", &block);
        assert_eq!(once, format!("set -g mouse on\n\n{block}"));
        assert_eq!(upsert_block(&once, &block), once, "idempotent");
        let with_tail = format!("{once}bind x y\n");
        let v2 = format!("{BEGIN}\nv2\n{END}\n");
        assert_eq!(upsert_block(&with_tail, &v2), format!("set -g mouse on\n\n{v2}bind x y\n"));
        assert_eq!(upsert_block("", &block), block);
    }

    #[test]
    fn snippets_quote_paths() {
        let t = tmux_snippet("/opt/my am/am", "/opt/my am/am-tui");
        assert!(t.contains("\"'/opt/my am/am-tui' --popup\""), "{t}");
        assert!(t.contains("#('/opt/my am/am' summary --format tmux)"), "{t}");
        assert!(t.contains("#{pane_current_path}") && t.starts_with(BEGIN) && t.trim_end().ends_with(END));
        let g = ghostty_snippet("/bin/am");
        assert!(g.contains("desktop-notifications = true") && g.contains("text:/bin/am sidebar\\r"), "{g}");
    }
}
