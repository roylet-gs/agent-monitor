//! Telemetry, in two independent pipelines:
//!
//! 1. **Anonymous usage and error reports** (`public.rs`, `schema.rs`,
//!    `spool.rs`, `upload.rs`; documented for users in `TELEMETRY.md`): opt-in
//!    (`telemetry` setting, environment opt-outs win), a fixed schema of small
//!    events every binary appends to a local spool, sent by am-tui (or
//!    `am telemetry flush`) as OTLP/JSON logs to a relay in front of Grafana
//!    Cloud. No message text, paths or names.
//! 2. **Your own collector** (below): the full OpenTelemetry stream.
//!
//! OpenTelemetry: logs, traces and metrics over OTLP/HTTP.
//!
//! [`init`] turns each signal on only when an OTLP endpoint is configured for
//! it through the standard variables (`OTEL_EXPORTER_OTLP_ENDPOINT`, or
//! `OTEL_EXPORTER_OTLP_{LOGS,TRACES,METRICS}_ENDPOINT`; headers, timeout and
//! the rest are read by the exporter itself), so a user can point am at their
//! own collector, e.g. `docker run -p 3000:3000 -p 4318:4318 grafana/otel-lgtm`.
//! `OTEL_SDK_DISABLED=true` turns it all off. Nothing is set up otherwise, and
//! the hook never exports (it runs per agent event and must stay offline).
//!
//! - logs: every event the log level lets through ([`crate::log`]), via
//!   `opentelemetry-appender-tracing`;
//! - traces: `tracing` spans (target `am`), via `tracing-opentelemetry`;
//! - metrics: the global meter provider, so instruments made after `init`
//!   (e.g. [`crate::perf`]'s `am.stage.duration`) export.
//!
//! Batches go out from the SDK's own threads through [`http::UreqClient`]. On
//! exit — the [`Guard`] dropping, or `std::process::exit` through `atexit` —
//! the providers are shut down, which flushes what is left.

pub mod http;
pub mod public;
pub mod schema;
pub mod spool;
pub mod upload;

pub use public::{Consent, consent, record};

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use opentelemetry::KeyValue;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_otlp::{LogExporter, MetricExporter, SpanExporter, WithHttpConfig};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::trace::SdkTracerProvider;
use tracing_subscriber::Layer;

use crate::settings::Settings;
use crate::{info, log, warn};

const MODULE: &str = "telemetry";
/// Bounds each export request, and each provider's flush on exit.
const TIMEOUT: Duration = Duration::from_secs(5);

/// Which binary is running: a resource attribute, and the hook never exports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Binary {
    Tui,
    Cli,
    Hook,
}

impl Binary {
    pub fn as_str(self) -> &'static str {
        match self {
            Binary::Tui => "am-tui",
            Binary::Cli => "am",
            Binary::Hook => "am-hook",
        }
    }
}

/// The signals [`init`] exports, from the environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Signals {
    pub logs: bool,
    pub traces: bool,
    pub metrics: bool,
}

impl Signals {
    pub fn from_env(var: impl Fn(&str) -> Option<String>) -> Signals {
        let set = |name: &str| var(name).is_some_and(|v| !v.trim().is_empty());
        if var("OTEL_SDK_DISABLED").is_some_and(|v| v.trim().eq_ignore_ascii_case("true")) {
            return Signals::default();
        }
        let any = set("OTEL_EXPORTER_OTLP_ENDPOINT");
        Signals {
            logs: any || set("OTEL_EXPORTER_OTLP_LOGS_ENDPOINT"),
            traces: any || set("OTEL_EXPORTER_OTLP_TRACES_ENDPOINT"),
            metrics: any || set("OTEL_EXPORTER_OTLP_METRICS_ENDPOINT"),
        }
    }

    pub fn any(self) -> bool {
        self.logs || self.traces || self.metrics
    }
}

#[derive(Default)]
struct Providers {
    logs: Option<SdkLoggerProvider>,
    traces: Option<SdkTracerProvider>,
    metrics: Option<SdkMeterProvider>,
}

static PROVIDERS: OnceLock<Providers> = OnceLock::new();
static METRICS: AtomicBool = AtomicBool::new(false);

/// Shuts the providers down when dropped; keep it alive in `main`.
#[must_use]
pub struct Guard(());

impl Drop for Guard {
    fn drop(&mut self) {
        shutdown();
    }
}

/// Start the usage reports (per `settings`) and the collector export (per the
/// environment) for `binary`. Call once, after [`log::init`] and before any
/// thread that records metrics starts.
pub fn init(binary: Binary, settings: &Settings) -> Guard {
    public::init(binary, settings);
    let signals = Signals::from_env(|name| std::env::var(name).ok());
    if binary == Binary::Hook || !signals.any() || PROVIDERS.get().is_some() {
        return Guard(());
    }
    let resource = resource(binary);
    let client = || http::UreqClient::new(TIMEOUT);
    let mut providers = Providers::default();
    let mut layers: Vec<log::ExtraLayer> = Vec::new();

    if signals.logs {
        match LogExporter::builder().with_http().with_http_client(client()).build() {
            Ok(exporter) => {
                let provider =
                    SdkLoggerProvider::builder().with_resource(resource.clone()).with_batch_exporter(exporter).build();
                layers.push(Box::new(OpenTelemetryTracingBridge::new(&provider)));
                providers.logs = Some(provider);
            }
            Err(e) => warn!(MODULE, "OTLP log exporter not started: {e}"),
        }
    }
    if signals.traces {
        match SpanExporter::builder().with_http().with_http_client(client()).build() {
            Ok(exporter) => {
                let provider =
                    SdkTracerProvider::builder().with_resource(resource.clone()).with_batch_exporter(exporter).build();
                let tracer = provider.tracer("am");
                layers.push(tracing_opentelemetry::layer().with_tracer(tracer).boxed());
                providers.traces = Some(provider);
            }
            Err(e) => warn!(MODULE, "OTLP span exporter not started: {e}"),
        }
    }
    if signals.metrics {
        match MetricExporter::builder().with_http().with_http_client(client()).build() {
            Ok(exporter) => {
                let provider =
                    SdkMeterProvider::builder().with_resource(resource.clone()).with_periodic_exporter(exporter).build();
                opentelemetry::global::set_meter_provider(provider.clone());
                METRICS.store(true, Ordering::Relaxed);
                providers.metrics = Some(provider);
            }
            Err(e) => warn!(MODULE, "OTLP metric exporter not started: {e}"),
        }
    }

    for layer in layers {
        log::add_layer(layer);
    }
    info!(
        MODULE,
        "OpenTelemetry export on for {}: logs={} traces={} metrics={}",
        binary.as_str(),
        providers.logs.is_some(),
        providers.traces.is_some(),
        providers.metrics.is_some()
    );
    if PROVIDERS.set(providers).is_ok() {
        // `std::process::exit` (the CLI's error paths) skips destructors.
        extern "C" fn at_exit() {
            shutdown();
        }
        unsafe {
            libc::atexit(at_exit);
        }
    }
    Guard(())
}

/// Whether a meter provider is exporting; instruments made before one is set
/// stay no-ops, so callers create theirs only once this is true.
pub fn metrics_on() -> bool {
    METRICS.load(Ordering::Relaxed)
}

/// Flush and stop every provider (once; later calls do nothing).
pub fn shutdown() {
    static DONE: AtomicBool = AtomicBool::new(false);
    let Some(p) = PROVIDERS.get() else { return };
    if DONE.swap(true, Ordering::SeqCst) {
        return;
    }
    if let Some(t) = &p.traces {
        let _ = t.shutdown_with_timeout(TIMEOUT);
    }
    if let Some(m) = &p.metrics {
        let _ = m.shutdown_with_timeout(TIMEOUT);
    }
    // Last, so the others' shutdown events still go out.
    if let Some(l) = &p.logs {
        let _ = l.shutdown_with_timeout(TIMEOUT);
    }
}

/// `service.name` defaults to `am` (`OTEL_SERVICE_NAME` and
/// `OTEL_RESOURCE_ATTRIBUTES` still apply on top).
fn resource(binary: Binary) -> Resource {
    Resource::builder()
        .with_service_name("am")
        .with_attributes([
            KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
            KeyValue::new("am.binary", binary.as_str()),
            KeyValue::new("os.type", os_type()),
            KeyValue::new("host.arch", host_arch()),
        ])
        .build()
}

/// OpenTelemetry's `os.type` values.
pub fn os_type() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        other => other,
    }
}

/// OpenTelemetry's `host.arch` values.
pub fn host_arch() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "amd64",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signals(vars: &[(&str, &str)]) -> Signals {
        Signals::from_env(|name| vars.iter().find(|(k, _)| *k == name).map(|(_, v)| v.to_string()))
    }

    #[test]
    fn nothing_exports_without_an_endpoint() {
        assert!(!signals(&[]).any());
        assert!(!signals(&[("OTEL_EXPORTER_OTLP_ENDPOINT", " ")]).any());
    }

    #[test]
    fn the_generic_endpoint_turns_every_signal_on() {
        let all = Signals { logs: true, traces: true, metrics: true };
        assert_eq!(signals(&[("OTEL_EXPORTER_OTLP_ENDPOINT", "http://localhost:4318")]), all);
    }

    #[test]
    fn a_signal_endpoint_turns_only_that_signal_on() {
        let s = signals(&[("OTEL_EXPORTER_OTLP_TRACES_ENDPOINT", "http://localhost:4318/v1/traces")]);
        assert_eq!(s, Signals { logs: false, traces: true, metrics: false });
    }

    #[test]
    fn sdk_disabled_wins() {
        let s = signals(&[("OTEL_EXPORTER_OTLP_ENDPOINT", "http://x"), ("OTEL_SDK_DISABLED", "TRUE")]);
        assert!(!s.any());
    }
}
