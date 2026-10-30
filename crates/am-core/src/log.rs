//! Append-only debug log in the same line format as the TS logger
//! (`[ISO] [LEVEL] [module] message`) so existing tooling keeps parsing it.
//!
//! Every log call is a `tracing` event: the `info!`/`warn!`/`debug!` macros
//! emit one with target `am` and the module as a `module` field (modules can
//! be runtime values, e.g. the breaker's source, and tracing targets can't),
//! and [`FileLayer`] writes it here. Other layers (OpenTelemetry) can sit
//! beside it on the same subscriber. The subscriber installs itself on the
//! first event, so code that logs before [`init`] still lands in the file.
//!
//! Differences from the TS logger, both fixes for problems seen in real logs:
//! - messages are flattened to a single line (raw stderr used to leave
//!   untimestamped continuation lines behind);
//! - rotation renames `debug.log` → `debug.log.1` instead of rewriting the
//!   last 1000 lines, so concurrent writers can't lose lines and history
//!   survives longer than an hour.

use std::fs::OpenOptions;
use std::io::Write;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Once, OnceLock};

use tracing::field::{Field, Visit};
use tracing::{Event, Metadata, Subscriber};
use tracing_subscriber::layer::{Context, Layer, Layered, SubscriberExt};
use tracing_subscriber::{Registry, reload};

use crate::paths;

/// The tracing target of every event the log macros emit.
pub const TARGET: &str = "am";

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

    fn from_tracing(level: tracing::Level) -> Option<Level> {
        match level {
            tracing::Level::ERROR => Some(Level::Error),
            tracing::Level::WARN => Some(Level::Warn),
            tracing::Level::INFO => Some(Level::Info),
            tracing::Level::DEBUG => Some(Level::Debug),
            tracing::Level::TRACE => None,
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
    install();
    // Callsites hit before init cached their interest against the default level.
    tracing::callsite::rebuild_interest_cache();
    rotate_if_needed();
}

/// What layers added with [`add_layer`] sit on: the registry plus [`FileLayer`].
pub type Base = Layered<FileLayer, Registry>;
/// A layer added after start-up (OpenTelemetry's bridge and span layers).
pub type ExtraLayer = Box<dyn Layer<Base> + Send + Sync>;

static EXTRA: OnceLock<reload::Handle<Vec<ExtraLayer>, Base>> = OnceLock::new();

/// Install the process-wide subscriber (once; the log macros call this).
pub fn install() {
    static INSTALLED: Once = Once::new();
    INSTALLED.call_once(|| {
        let (extra, handle) = reload::Layer::new(Vec::<ExtraLayer>::new());
        let _ = EXTRA.set(handle);
        let _ = tracing::subscriber::set_global_default(tracing_subscriber::registry().with(FileLayer).with(extra));
    });
}

/// Add a layer beside [`FileLayer`]. It only sees what the log level lets
/// through, since [`FileLayer`] filters for the whole subscriber.
pub fn add_layer(layer: ExtraLayer) {
    install();
    if let Some(handle) = EXTRA.get() {
        let _ = handle.modify(|layers| layers.push(layer));
    }
}

/// Writes each event at or above the configured level to debug.log.
pub struct FileLayer;

impl<S: Subscriber> Layer<S> for FileLayer {
    fn enabled(&self, meta: &Metadata<'_>, _: Context<'_, S>) -> bool {
        Level::from_tracing(*meta.level()).is_some_and(enabled)
    }

    fn on_event(&self, event: &Event<'_>, _: Context<'_, S>) {
        let Some(level) = Level::from_tracing(*event.metadata().level()) else { return };
        let mut fields = Fields::default();
        event.record(&mut fields);
        let module = fields.module.as_deref().unwrap_or(event.metadata().target());
        log(level, module, &fields.message);
    }
}

/// The `module` and `message` of an event; other fields are appended to the
/// message as `key=value` so structured events still read in the file.
#[derive(Default)]
struct Fields {
    module: Option<String>,
    message: String,
}

impl Visit for Fields {
    fn record_str(&mut self, field: &Field, value: &str) {
        match field.name() {
            "module" => self.module = Some(value.to_string()),
            "message" => self.message.insert_str(0, value),
            name => self.push(name, value),
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        match field.name() {
            "module" => self.module = Some(format!("{value:?}")),
            "message" => self.message.insert_str(0, &format!("{value:?}")),
            name => self.push(name, &format!("{value:?}")),
        }
    }
}

impl Fields {
    fn push(&mut self, name: &str, value: &str) {
        self.message.push(' ');
        self.message.push_str(name);
        self.message.push('=');
        self.message.push_str(value);
    }
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
    let line = format_line(time::OffsetDateTime::now_utc(), level, module, message);
    let dir = paths::app_dir();
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(paths::log_path()) {
        // One write call per line: O_APPEND keeps concurrent hook processes from interleaving.
        let _ = f.write_all(line.as_bytes());
    }
}

/// One debug.log line, newline included.
fn format_line(at: time::OffsetDateTime, level: Level, module: &str, message: &str) -> String {
    let ts = at
        .format(time::macros::format_description!(
            "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z"
        ))
        .unwrap_or_default();
    format!("[{ts}] [{}] [{module}] {}\n", level.label(), flatten(message))
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

// Each takes an optional `kind = "…"` after the module: a short, fixed error
// kind (`rate_limited`, `network`) that the anonymous usage reports may carry
// (the message never leaves the machine). In debug.log it reads `kind=…`.
#[macro_export]
macro_rules! debug {
    ($module:expr, kind = $kind:expr, $($arg:tt)*) => {
        $crate::log_event!($crate::tracing::Level::DEBUG, $module, kind = $kind, $($arg)*)
    };
    ($module:expr, $($arg:tt)*) => {
        $crate::log_event!($crate::tracing::Level::DEBUG, $module, $($arg)*)
    };
}

#[macro_export]
macro_rules! info {
    ($module:expr, kind = $kind:expr, $($arg:tt)*) => {
        $crate::log_event!($crate::tracing::Level::INFO, $module, kind = $kind, $($arg)*)
    };
    ($module:expr, $($arg:tt)*) => {
        $crate::log_event!($crate::tracing::Level::INFO, $module, $($arg)*)
    };
}

#[macro_export]
macro_rules! warn {
    ($module:expr, kind = $kind:expr, $($arg:tt)*) => {
        $crate::log_event!($crate::tracing::Level::WARN, $module, kind = $kind, $($arg)*)
    };
    ($module:expr, $($arg:tt)*) => {
        $crate::log_event!($crate::tracing::Level::WARN, $module, $($arg)*)
    };
}

/// One tracing event for a log line; the message is only formatted when a
/// layer is interested in the level.
#[doc(hidden)]
#[macro_export]
macro_rules! log_event {
    ($level:expr, $module:expr, kind = $kind:expr, $($arg:tt)*) => {{
        $crate::log::install();
        $crate::tracing::event!(target: "am", $level, module = $module, kind = $kind, "{}", format_args!($($arg)*));
    }};
    ($level:expr, $module:expr, $($arg:tt)*) => {{
        $crate::log::install();
        $crate::tracing::event!(target: "am", $level, module = $module, "{}", format_args!($($arg)*));
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::{Arc, Mutex};

    /// Records what [`FileLayer`] would write, without the file.
    struct Capture(Arc<Mutex<Vec<(Level, String, String)>>>);

    impl<S: Subscriber> Layer<S> for Capture {
        fn on_event(&self, event: &Event<'_>, _: Context<'_, S>) {
            let mut fields = Fields::default();
            event.record(&mut fields);
            let level = Level::from_tracing(*event.metadata().level()).unwrap();
            let module = fields.module.unwrap_or_else(|| event.metadata().target().to_string());
            self.0.lock().unwrap().push((level, module, fields.message));
        }
    }

    fn captured(f: impl FnOnce()) -> Vec<(Level, String, String)> {
        let lines = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::registry().with(Capture(lines.clone()));
        tracing::subscriber::with_default(subscriber, f);
        lines.lock().unwrap().clone()
    }

    #[test]
    fn macros_emit_events_with_their_module_and_message() {
        let source: &str = "github";
        let e = "timed out";
        let lines = captured(|| {
            crate::info!("sync", "synced {} worktree(s)", 3);
            crate::warn!(source, "{source} failing: {e}");
            crate::debug!(crate::perf::MODULE, "p95 {}ms", 12);
            crate::warn!("github", kind = "rate_limited", "search failed: {e}");
        });
        assert_eq!(
            lines,
            vec![
                (Level::Info, "sync".into(), "synced 3 worktree(s)".into()),
                (Level::Warn, "github".into(), "github failing: timed out".into()),
                (Level::Debug, "perf".into(), "p95 12ms".into()),
                (Level::Warn, "github".into(), "search failed: timed out kind=rate_limited".into()),
            ]
        );
    }

    #[test]
    fn native_tracing_events_use_their_target_and_keep_extra_fields() {
        let lines = captured(|| tracing::warn!(target: "reviews", kind = "rate_limited", "search failed"));
        assert_eq!(lines, vec![(Level::Warn, "reviews".into(), "search failed kind=rate_limited".into())]);
    }

    #[test]
    fn lines_keep_the_1x_format() {
        let at = time::macros::datetime!(2026-10-30 09:05:01.234 UTC);
        assert_eq!(
            format_line(at, Level::Warn, "github", "gh failed\ncheck your internet"),
            "[2026-10-30T09:05:01.234Z] [WARN] [github] gh failed ⏎ check your internet\n"
        );
    }

    #[test]
    fn flatten_joins_multiline_messages() {
        assert_eq!(flatten("one line"), "one line");
        assert_eq!(
            flatten("gh failed: error connecting\ncheck your internet\n\n"),
            "gh failed: error connecting ⏎ check your internet"
        );
    }
}
