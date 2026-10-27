//! Append-only debug log in the same line format as the TS logger
//! (`[ISO] [LEVEL] [module] message`) so existing tooling keeps parsing it.
//!
//! Differences from the TS logger, both fixes for problems seen in real logs:
//! - messages are flattened to a single line (raw stderr used to leave
//!   untimestamped continuation lines behind);
//! - rotation renames `debug.log` → `debug.log.1` instead of rewriting the
//!   last 1000 lines, so concurrent writers can't lose lines and history
//!   survives longer than an hour.

use std::fs::OpenOptions;
use std::io::Write;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::paths;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

impl Level {
    pub fn parse(s: &str) -> Option<Level> {
        match s {
            "debug" => Some(Level::Debug),
            "info" => Some(Level::Info),
            "warn" => Some(Level::Warn),
            "error" => Some(Level::Error),
            _ => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Level::Debug => "DEBUG",
            Level::Info => "INFO",
            Level::Warn => "WARN",
            Level::Error => "ERROR",
        }
    }
}

struct Config {
    level: Level,
    max_bytes: u64,
}

static CONFIG: OnceLock<Config> = OnceLock::new();
static SILENT: AtomicBool = AtomicBool::new(false);

/// Stop writing log lines for the rest of the process. The other crates'
/// unit tests link am-core without its `cfg(test)`, so they call this (see
/// [`silence_unit_tests!`]) to stay out of the real debug.log too.
pub fn silence() {
    SILENT.store(true, Ordering::Relaxed);
}

pub fn init(level: Level, max_log_size_mb: u64) {
    let _ = CONFIG.set(Config { level, max_bytes: max_log_size_mb.max(1) * 1024 * 1024 });
    rotate_if_needed();
}

fn config() -> &'static Config {
    CONFIG.get_or_init(|| Config { level: Level::Info, max_bytes: 2 * 1024 * 1024 })
}

pub fn enabled(level: Level) -> bool {
    level >= config().level
}

pub fn log(level: Level, module: &str, message: &str) {
    // Unit tests exercise warn/info paths; never let them write to the real
    // ~/.agent-monitor/debug.log.
    if cfg!(test) || SILENT.load(Ordering::Relaxed) || !enabled(level) {
        return;
    }
    let ts = time::OffsetDateTime::now_utc()
        .format(time::macros::format_description!(
            "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z"
        ))
        .unwrap_or_default();
    let flat = flatten(message);
    let line = format!("[{ts}] [{}] [{module}] {flat}\n", level.label());
    let dir = paths::app_dir();
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(paths::log_path()) {
        // One write call per line: O_APPEND keeps concurrent hook processes from interleaving.
        let _ = f.write_all(line.as_bytes());
    }
}

/// Collapse newlines so one log call is always exactly one log line.
pub fn flatten(message: &str) -> String {
    if !message.contains(['\n', '\r']) {
        return message.to_string();
    }
    message
        .split(['\n', '\r'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ⏎ ")
}

fn rotate_if_needed() {
    let path = paths::log_path();
    let Ok(meta) = std::fs::metadata(&path) else { return };
    if meta.len() <= config().max_bytes {
        return;
    }
    // rename is atomic; a racing writer either lands in the old file (now .1)
    // or creates a fresh debug.log — no lines are lost either way.
    let _ = std::fs::rename(&path, paths::rotated_log_path());
}

/// In a crate root: silence the log before any unit test runs (a static
/// initializer, so no test can log first). Only in that crate's `cfg(test)`
/// build; its binaries (and their integration tests) keep logging.
#[macro_export]
macro_rules! silence_unit_tests {
    () => {
        #[cfg(test)]
        #[used]
        #[cfg_attr(target_os = "macos", unsafe(link_section = "__DATA,__mod_init_func"))]
        #[cfg_attr(not(target_os = "macos"), unsafe(link_section = ".init_array"))]
        static SILENCE_LOG_FOR_UNIT_TESTS: extern "C" fn() = {
            extern "C" fn silence() {
                $crate::log::silence();
            }
            silence
        };
    };
}

#[macro_export]
macro_rules! debug {
    ($module:expr, $($arg:tt)*) => {
        if $crate::log::enabled($crate::log::Level::Debug) {
            $crate::log::log($crate::log::Level::Debug, $module, &format!($($arg)*));
        }
    };
}

#[macro_export]
macro_rules! info {
    ($module:expr, $($arg:tt)*) => {
        $crate::log::log($crate::log::Level::Info, $module, &format!($($arg)*))
    };
}

#[macro_export]
macro_rules! warn {
    ($module:expr, $($arg:tt)*) => {
        $crate::log::log($crate::log::Level::Warn, $module, &format!($($arg)*))
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flatten_joins_multiline_messages() {
        assert_eq!(flatten("one line"), "one line");
        assert_eq!(
            flatten("gh failed: error connecting\ncheck your internet\n\n"),
            "gh failed: error connecting ⏎ check your internet"
        );
    }
}
