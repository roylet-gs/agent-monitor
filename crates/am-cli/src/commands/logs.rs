//! `am logs` (src/commands/logs.ts): tail with level/module filters, follow, clear,
//! and `--open` in the editor.

use std::io::{Read, Seek, SeekFrom};
use std::time::Duration;

use am_core::launch::{self, IdeChoice, LogView};
use am_core::{info, paths, settings};

pub struct Options {
    pub lines: usize,
    pub follow: bool,
    pub level: Option<String>,
    pub module: Option<String>,
    pub clear: bool,
    pub open: bool,
}

/// Levels shown for a `--level` filter (that level and above).
fn allowed_levels(level: &str) -> Option<&'static [&'static str]> {
    Some(match level.to_lowercase().as_str() {
        "debug" => &["DEBUG", "INFO", "WARN", "ERROR"],
        "info" => &["INFO", "WARN", "ERROR"],
        "warn" => &["WARN", "ERROR"],
        "error" => &["ERROR"],
        _ => return None,
    })
}

pub fn keep_line(line: &str, level: Option<&str>, module: Option<&str>) -> bool {
    if let Some(allowed) = level.and_then(allowed_levels)
        && !allowed.iter().any(|l| line.contains(&format!("[{l}]")))
    {
        return false;
    }
    if let Some(m) = module
        && !line.contains(&format!("[{m}]"))
    {
        return false;
    }
    true
}

/// `--open`: debug.log in the `ide` (`$VISUAL`/`$EDITOR` here for `terminal`),
/// or the log folder in the file manager once it has rotated.
fn open() {
    let result = match launch::log_view(&paths::log_files()) {
        LogView::Missing => {
            println!("No log file found at {}", paths::log_path().display());
            return;
        }
        LogView::Reveal(path) => launch::reveal_in_file_manager(&path).map(|()| format!("Opened the log folder {}", paths::app_dir().display())),
        LogView::Edit(path) => {
            let s = settings::load();
            let ide = IdeChoice::from_setting(&s.ide);
            let r = if ide == IdeChoice::Terminal { edit_here(&path) } else { launch::open_in_editor(&path, ide, &s.ide_command) };
            r.map(|()| format!("Opened {}", path.display()))
        }
    };
    match result {
        Ok(msg) => {
            info!("cli", "logs --open: {msg}");
            println!("{msg}");
        }
        Err(e) => crate::fail(&e),
    }
}

/// `$VISUAL`/`$EDITOR` (default `vi`) on `path` in this terminal.
fn edit_here(path: &std::path::Path) -> Result<(), String> {
    let editor = std::env::var("VISUAL").or_else(|_| std::env::var("EDITOR")).unwrap_or_else(|_| "vi".into());
    // $EDITOR may carry arguments (e.g. `code --wait`), so run it via the shell.
    let status = std::process::Command::new("sh").arg("-c").arg(format!("{editor} \"$1\"")).arg("sh").arg(path).status();
    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!("{editor} exited with {s}")),
        Err(e) => Err(format!("Couldn't run {editor}: {e}")),
    }
}

pub fn run(o: Options) {
    if o.open {
        return open();
    }
    let path = paths::log_path();
    if o.clear {
        if path.exists() {
            if let Err(e) = std::fs::OpenOptions::new().write(true).truncate(true).open(&path) {
                crate::fail(&format!("Failed to clear {}: {e}", path.display()));
            }
            println!("Log file cleared.");
        } else {
            println!("No log file found.");
        }
        return;
    }
    let Ok(bytes) = std::fs::read(&path) else {
        println!("No log file found at {}", path.display());
        return;
    };
    let content = String::from_utf8_lossy(&bytes);
    let (level, module) = (o.level.as_deref(), o.module.as_deref());
    let filtered: Vec<&str> = content.split('\n').filter(|l| !l.is_empty() && keep_line(l, level, module)).collect();
    for line in &filtered[filtered.len().saturating_sub(o.lines)..] {
        println!("{line}");
    }
    if !o.follow {
        return;
    }

    println!("--- following log (Ctrl+C to stop) ---");
    let mut offset = bytes.len() as u64;
    let mut partial = String::new();
    loop {
        std::thread::sleep(Duration::from_millis(300));
        let Ok(mut f) = std::fs::File::open(&path) else { continue };
        let len = f.metadata().map(|m| m.len()).unwrap_or(0);
        if len < offset {
            // Truncated or rotated: start over from the top of the new file.
            offset = 0;
            partial.clear();
        }
        if len == offset || f.seek(SeekFrom::Start(offset)).is_err() {
            continue;
        }
        let mut buf = Vec::new();
        if f.read_to_end(&mut buf).is_err() {
            continue;
        }
        offset += buf.len() as u64;
        partial.push_str(&String::from_utf8_lossy(&buf));
        // Only print complete lines; keep a trailing fragment for the next poll.
        let complete = partial.rfind('\n').map(|i| i + 1).unwrap_or(0);
        let ready: String = partial.drain(..complete).collect();
        for line in ready.split('\n').filter(|l| !l.is_empty() && keep_line(l, level, module)) {
            println!("{line}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::keep_line;

    const LINE: &str = "[2026-10-11T09:00:00.000Z] [WARN] [github] offline";

    #[test]
    fn level_filter_includes_higher_levels() {
        assert!(keep_line(LINE, Some("info"), None));
        assert!(keep_line(LINE, Some("WARN"), None));
        assert!(!keep_line(LINE, Some("error"), None));
        assert!(keep_line(LINE, Some("bogus"), None));
    }

    #[test]
    fn module_filter_matches_the_tag() {
        assert!(keep_line(LINE, None, Some("github")));
        assert!(!keep_line(LINE, None, Some("linear")));
    }
}
