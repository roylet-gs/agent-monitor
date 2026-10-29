//! The anonymous, opt-in pipeline: consent, the install id, recording
//! [`Event`]s into the spool, WARN/ERROR log events as `am.error` (where, not
//! what) and panics as `am.crash`. Nothing is recorded unless [`consent`] is
//! [`Consent::On`]; sending is `upload.rs`.

use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, Once, OnceLock};

use tracing::field::{Field, Visit};
use tracing::{Event as TracingEvent, Subscriber};
use tracing_subscriber::layer::{Context, Layer};

use super::schema::Event;
use super::{Binary, spool};
use crate::settings::{Settings, TelemetryChoice};
use crate::{info, log, paths};

const MODULE: &str = "telemetry";
/// `am.error` events per log callsite per process, so a loop can't fill the spool.
const MAX_PER_CALLSITE: u32 = 20;

/// Whether the anonymous reports are recorded and sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Consent {
    On,
    /// Off, and why (shown by `am telemetry status`).
    Off(&'static str),
    /// Not decided yet: nothing is recorded until the user picks.
    Ask,
}

/// Environment opt-outs win over the setting: `DO_NOT_TRACK` (any value but
/// `0`), `AM_TELEMETRY=0|off|false`, and `CI`.
pub fn consent(settings: &Settings, var: impl Fn(&str) -> Option<String>) -> Consent {
    let set = |name: &str| var(name).map(|v| v.trim().to_ascii_lowercase()).filter(|v| !v.is_empty());
    if set("DO_NOT_TRACK").is_some_and(|v| v != "0" && v != "false") {
        return Consent::Off("DO_NOT_TRACK is set");
    }
    if set("AM_TELEMETRY").is_some_and(|v| matches!(v.as_str(), "0" | "off" | "false")) {
        return Consent::Off("AM_TELEMETRY is off");
    }
    if set("CI").is_some_and(|v| v != "0" && v != "false") {
        return Consent::Off("running in CI");
    }
    match settings.telemetry() {
        TelemetryChoice::On => Consent::On,
        TelemetryChoice::Off => Consent::Off("turned off in settings"),
        TelemetryChoice::Ask => Consent::Ask,
    }
}

static ON: AtomicBool = AtomicBool::new(false);
static BINARY: OnceLock<&'static str> = OnceLock::new();

pub fn dir() -> PathBuf {
    paths::app_dir().join("telemetry")
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// The running binary's name for events (`am` before [`init`]).
pub fn binary() -> &'static str {
    BINARY.get().copied().unwrap_or("am")
}

/// Whether events are being recorded right now.
pub fn is_on() -> bool {
    ON.load(Ordering::Relaxed)
}

/// Record `event` if reports are on.
pub fn record(event: Event) {
    if is_on() {
        spool::append(&dir(), &event);
    }
}

/// Start recording per `settings` + the environment: the `am.error` layer and
/// the crash hook go in once and check [`is_on`] each time.
pub fn init(binary: Binary, settings: &Settings) {
    let _ = BINARY.set(binary.as_str());
    static INSTALLED: Once = Once::new();
    INSTALLED.call_once(|| {
        log::add_layer(Box::new(SpoolLayer::default()));
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if let Some(at) = info.location() {
                record(Event::crash(self::binary(), now_ms(), at.file(), at.line()));
            }
            previous(info);
        }));
    });
    apply(settings);
}

/// Follow a settings change (the TUI reloads settings live): turning reports
/// off deletes whatever was waiting to be sent.
pub fn apply(settings: &Settings) {
    let consent = consent(settings, |name| std::env::var(name).ok());
    let on = consent == Consent::On;
    let was = ON.swap(on, Ordering::Relaxed);
    // Off however it got there (Settings, `am settings set`, an edited file,
    // `DO_NOT_TRACK`): whatever was waiting goes.
    if matches!(consent, Consent::Off(_)) && spool::has_pending(&dir()) {
        spool::clear(&dir());
        info!(MODULE, "Usage reports off ({consent:?}); pending reports deleted");
    } else if on && !was {
        info!(MODULE, "Usage reports on");
    }
}

/// The random install id (a UUID in `telemetry/install-id`), made on first
/// use. It ties reports from one install together and nothing else.
pub fn install_id() -> String {
    let path = dir().join("install-id");
    if let Ok(id) = std::fs::read_to_string(&path)
        && super::schema::token(id.trim())
    {
        return id.trim().to_string();
    }
    let id = uuid::Uuid::new_v4().to_string();
    let _ = std::fs::create_dir_all(dir());
    let _ = std::fs::write(&path, &id);
    id
}

/// Forget the install id; the next report starts a new one.
pub fn reset_install_id() {
    let _ = std::fs::remove_file(dir().join("install-id"));
}

/// Turns WARN/ERROR log events into `am.error`: level, module, source file and
/// line, and a `kind` field when the call gives one. The message is never read.
#[derive(Default)]
struct SpoolLayer {
    seen: Mutex<HashMap<(&'static str, u32), u32>>,
}

impl<S: Subscriber> Layer<S> for SpoolLayer {
    fn on_event(&self, event: &TracingEvent<'_>, _: Context<'_, S>) {
        if !is_on() || *event.metadata().level() > tracing::Level::WARN {
            return;
        }
        let meta = event.metadata();
        if let (Some(file), Some(line), Ok(mut seen)) = (meta.file(), meta.line(), self.seen.lock()) {
            let n = seen.entry((file, line)).or_default();
            *n += 1;
            if *n > MAX_PER_CALLSITE {
                return;
            }
        }
        if let Some(e) = error_event(event, now_ms()) {
            record(e);
        }
    }
}

/// The `am.error` for a WARN/ERROR log event (`None` for other levels).
fn error_event(event: &TracingEvent<'_>, ts: u64) -> Option<Event> {
    let meta = event.metadata();
    let level = match *meta.level() {
        tracing::Level::ERROR => "ERROR",
        tracing::Level::WARN => "WARN",
        _ => return None,
    };
    let mut fields = ErrorFields::default();
    event.record(&mut fields);
    let module = fields.module.as_deref().unwrap_or(meta.target());
    Some(Event::error(binary(), ts, level, module, meta.file()?, meta.line()?, fields.kind.as_deref()))
}

#[derive(Default)]
struct ErrorFields {
    module: Option<String>,
    kind: Option<String>,
}

impl Visit for ErrorFields {
    fn record_str(&mut self, field: &Field, value: &str) {
        match field.name() {
            "module" => self.module = Some(value.into()),
            "kind" => self.kind = Some(value.into()),
            _ => {}
        }
    }

    fn record_debug(&mut self, _: &Field, _: &dyn fmt::Debug) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn consent_with(setting: &str, vars: &[(&str, &str)]) -> Consent {
        let s = Settings::parse(&format!(r#"{{"telemetry":"{setting}"}}"#));
        consent(&s, |name| vars.iter().find(|(k, _)| *k == name).map(|(_, v)| v.to_string()))
    }

    #[test]
    fn the_setting_decides_by_default() {
        assert_eq!(consent_with("on", &[]), Consent::On);
        assert_eq!(consent_with("off", &[]), Consent::Off("turned off in settings"));
        assert_eq!(consent_with("ask", &[]), Consent::Ask);
        assert_eq!(consent_with("bogus", &[]), Consent::Ask);
    }

    #[test]
    fn environment_opt_outs_win() {
        assert_eq!(consent_with("on", &[("DO_NOT_TRACK", "1")]), Consent::Off("DO_NOT_TRACK is set"));
        assert_eq!(consent_with("on", &[("DO_NOT_TRACK", "0")]), Consent::On);
        assert_eq!(consent_with("on", &[("AM_TELEMETRY", "off")]), Consent::Off("AM_TELEMETRY is off"));
        assert_eq!(consent_with("on", &[("CI", "true")]), Consent::Off("running in CI"));
        assert_eq!(consent_with("on", &[("CI", "")]), Consent::On);
    }

    #[test]
    fn error_events_carry_where_not_what() {
        let dir = tempfile::tempdir().unwrap();
        let subscriber =
            tracing_subscriber::layer::SubscriberExt::with(tracing_subscriber::registry(), Capture(dir.path().to_path_buf()));
        tracing::subscriber::with_default(subscriber, || {
            let path = "/Users/someone/secret-repo";
            crate::warn!("github", "gh failed for {path}");
            tracing::error!(target: "reviews", kind = "rate_limited", "search failed for {path}");
            crate::info!("sync", "not an error");
        });
        let events = spool::pending(dir.path());
        assert_eq!(events.len(), 2);
        let text = std::fs::read_to_string(spool::spool_path(dir.path())).unwrap();
        assert!(!text.contains("secret") && !text.contains("failed"), "{text}");
        assert_eq!(events[0].attrs["module"], super::super::schema::Attr::Str("github".into()));
        assert_eq!(events[1].attrs["error.kind"], super::super::schema::Attr::Str("rate_limited".into()));
        assert_eq!(events[1].attrs["code.filepath"], super::super::schema::Attr::Str(file!().into()));
    }

    /// [`error_event`] into a test directory, regardless of [`is_on`].
    struct Capture(PathBuf);

    impl<S: Subscriber> Layer<S> for Capture {
        fn on_event(&self, event: &TracingEvent<'_>, _: Context<'_, S>) {
            if let Some(e) = error_event(event, 1) {
                spool::append(&self.0, &e);
            }
        }
    }
}
