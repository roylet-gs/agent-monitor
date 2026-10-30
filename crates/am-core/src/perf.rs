//! Runtime performance monitor: how long each stage of the app takes, and a
//! warning when one gets slow.
//!
//! Each long-lived thread (the TUI loop, the collector, the integrations
//! thread) owns a [`Perf`] and records stage timings into it; there is no
//! global state and no locking. Logging follows the `breaker.rs` rules:
//! - one INFO summary line per thread every [`REPORT_EVERY`] with
//!   `n`/`p50`/`p95`/`max` per stage (what `am perf` reads back);
//! - WARN only when a stage's p95 goes over its [`budget`] (ok → slow), INFO
//!   when it comes back under; nothing per sample.
//!
//! Every sample also goes to the `am.stage.duration` histogram (seconds,
//! attribute `stage`) when OpenTelemetry metrics are on ([`crate::telemetry`]).
//!
//! Time is passed in explicitly (`record_at`) so the rules are unit-testable.

use std::collections::VecDeque;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use opentelemetry::KeyValue;
use opentelemetry::metrics::Histogram;

use crate::{info, warn};

pub const MODULE: &str = "perf";
/// How often each thread writes its summary line.
pub const REPORT_EVERY: Duration = Duration::from_secs(300);
/// How often each thread records its `am.perf` usage-report events (when
/// reports are on): hourly, from every sample in the hour, to keep the
/// volume down (a busy install produced ~130 events an hour at 5 minutes).
pub const USAGE_REPORT_EVERY: Duration = Duration::from_secs(3600);
/// Samples kept per stage for the percentiles.
const WINDOW: usize = 128;
/// A stage is only judged once it has this many samples in its window: with
/// fewer, p95 is just the max, and one startup outlier would trip it.
const MIN_SAMPLES: usize = 20;

/// Per-stage budgets: p95 above this is "slow". Unknown stages have none.
pub const BUDGETS: &[(&str, u64)] = &[
    ("ui.frame", 16),
    ("ui.input", 50),
    ("collector.cycle", 500),
    ("collector.sync", 300),
    ("collector.git", 3_000),
    ("collector.context", 100),
    ("collector.procscan", 50),
    ("collector.usage", 100),
    ("collector.stacks", 1_000),
    ("integrations.github", 10_000),
    ("integrations.linear", 10_000),
    ("integrations.reviews", 10_000),
    ("hook.event", 50),
];

/// Bucket bounds (seconds) for `am.stage.duration`, spanning the budgets.
const BUCKETS: &[f64] = &[0.001, 0.0025, 0.005, 0.01, 0.016, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0];

/// Created on first use, which is after `telemetry::init` set the meter provider.
fn stage_histogram() -> &'static Histogram<f64> {
    static HISTOGRAM: OnceLock<Histogram<f64>> = OnceLock::new();
    HISTOGRAM.get_or_init(|| {
        opentelemetry::global::meter("am")
            .f64_histogram("am.stage.duration")
            .with_description("How long each stage of am takes")
            .with_unit("s")
            .with_boundaries(BUCKETS.to_vec())
            .build()
    })
}

pub fn budget(stage: &str) -> Option<Duration> {
    BUDGETS.iter().find(|(s, _)| *s == stage).map(|(_, ms)| Duration::from_millis(*ms))
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Summary {
    pub n: usize,
    pub p50: Duration,
    pub p95: Duration,
    pub max: Duration,
}

/// Percentiles of `samples` (nearest-rank). `None` when empty.
pub fn summarize(samples: impl IntoIterator<Item = Duration>) -> Option<Summary> {
    let mut v: Vec<Duration> = samples.into_iter().collect();
    if v.is_empty() {
        return None;
    }
    v.sort_unstable();
    let at = |q: f64| v[(((v.len() as f64) * q).ceil() as usize).clamp(1, v.len()) - 1];
    Some(Summary { n: v.len(), p50: at(0.5), p95: at(0.95), max: v[v.len() - 1] })
}

#[derive(Debug)]
struct Stage {
    name: &'static str,
    window: VecDeque<Duration>,
    /// Samples since the last summary line.
    period: Vec<Duration>,
    /// Samples since the last `am.perf` usage report (budgeted stages only).
    hour: Vec<Duration>,
    slow: bool,
}

/// One thread's stage timings.
#[derive(Debug)]
pub struct Perf {
    stages: Vec<Stage>,
    last_report: Instant,
    last_usage_report: Instant,
}

impl Default for Perf {
    fn default() -> Self {
        Perf::new()
    }
}

impl Perf {
    pub fn new() -> Self {
        let now = Instant::now();
        Perf { stages: Vec::new(), last_report: now, last_usage_report: now }
    }

    pub fn record(&mut self, stage: &'static str, elapsed: Duration) {
        self.record_at(stage, elapsed, Instant::now());
    }

    /// Time `f` as `stage`.
    pub fn time<T>(&mut self, stage: &'static str, f: impl FnOnce() -> T) -> T {
        let started = Instant::now();
        let out = f();
        self.record(stage, started.elapsed());
        out
    }

    pub fn record_at(&mut self, stage: &'static str, elapsed: Duration, now: Instant) {
        let i = match self.stages.iter().position(|s| s.name == stage) {
            Some(i) => i,
            None => {
                self.stages.push(Stage {
                    name: stage,
                    window: VecDeque::with_capacity(WINDOW),
                    period: Vec::new(),
                    hour: Vec::new(),
                    slow: false,
                });
                self.stages.len() - 1
            }
        };
        if crate::telemetry::metrics_on() {
            stage_histogram().record(elapsed.as_secs_f64(), &[KeyValue::new("stage", stage)]);
        }
        let s = &mut self.stages[i];
        if s.window.len() == WINDOW {
            s.window.pop_front();
        }
        s.window.push_back(elapsed);
        s.period.push(elapsed);
        if budget(stage).is_some() && crate::telemetry::public::is_on() {
            s.hour.push(elapsed);
        }
        if let (Some(budget), true) = (budget(stage), s.window.len() >= MIN_SAMPLES)
            && let Some(sum) = summarize(s.window.iter().copied())
        {
            let slow = sum.p95 > budget;
            if slow && !s.slow {
                warn!(MODULE, "{stage} slow: p95 {} > budget {} (n={}, max {})", ms(sum.p95), ms(budget), sum.n, ms(sum.max));
            } else if !slow && s.slow {
                info!(MODULE, "{stage} back under budget: p95 {} ≤ {} (n={})", ms(sum.p95), ms(budget), sum.n);
            }
            s.slow = slow;
        }
        if now.duration_since(self.last_report) >= REPORT_EVERY {
            self.report(now);
        }
        if now.duration_since(self.last_usage_report) >= USAGE_REPORT_EVERY {
            self.report_usage(now);
        }
    }

    /// The summary line for the samples since the last one (`None` if there were none).
    pub fn summary_line(&self) -> Option<String> {
        let parts: Vec<String> = self
            .stages
            .iter()
            .filter_map(|s| {
                let sum = summarize(s.period.iter().copied())?;
                let budget = budget(s.name).map(|b| format!(" budget={}", ms(b))).unwrap_or_default();
                Some(format!("{} n={} p50={} p95={} max={}{budget}", s.name, sum.n, ms(sum.p50), ms(sum.p95), ms(sum.max)))
            })
            .collect();
        (!parts.is_empty()).then(|| parts.join("; "))
    }

    /// Write the summary line and start a new period.
    pub fn report(&mut self, now: Instant) {
        if let Some(line) = self.summary_line() {
            info!(MODULE, "{line}");
        }
        for s in &mut self.stages {
            s.period.clear();
        }
        self.last_report = now;
    }

    /// The `am.perf` usage-report events for the samples since the last ones:
    /// one per budgeted stage that has any.
    pub fn usage_events(&self, ts: u64) -> Vec<crate::telemetry::schema::Event> {
        let us = |d: Duration| d.as_micros() as u64;
        self.stages
            .iter()
            .filter_map(|s| {
                let sum = summarize(s.hour.iter().copied())?;
                Some(crate::telemetry::schema::Event::perf(
                    crate::telemetry::public::binary(),
                    ts,
                    s.name,
                    sum.n as u64,
                    us(sum.p50),
                    us(sum.p95),
                    us(sum.max),
                ))
            })
            .collect()
    }

    /// Record the `am.perf` events (when reports are on) and start a new hour.
    pub fn report_usage(&mut self, now: Instant) {
        if crate::telemetry::public::is_on() {
            for e in self.usage_events(crate::telemetry::public::now_ms()) {
                crate::telemetry::record(e);
            }
        }
        for s in &mut self.stages {
            s.hour.clear();
        }
        self.last_usage_report = now;
    }

    /// On exit: the last summary line, and the part of the hour so far.
    pub fn finish(&mut self, now: Instant) {
        self.report(now);
        self.report_usage(now);
    }

    /// Stages currently over budget.
    pub fn slow_stages(&self) -> Vec<&'static str> {
        self.stages.iter().filter(|s| s.slow).map(|s| s.name).collect()
    }
}

/// `1.23ms` / `8.10s` — what summary lines carry and [`parse_ms`] reads back.
pub fn ms(d: Duration) -> String {
    let ms = d.as_secs_f64() * 1000.0;
    if ms >= 1000.0 { format!("{:.2}s", ms / 1000.0) } else { format!("{ms:.2}ms") }
}

/// Reads a duration as `Debug`/[`ms`] print it: `8.1s`, `12.5ms`, `830µs`, `40ns`.
pub fn parse_duration(s: &str) -> Option<Duration> {
    let s = s.trim();
    let (num, mul) = if let Some(n) = s.strip_suffix("ms") {
        (n, 1e-3)
    } else if let Some(n) = s.strip_suffix("µs").or_else(|| s.strip_suffix("us")) {
        (n, 1e-6)
    } else if let Some(n) = s.strip_suffix("ns") {
        (n, 1e-9)
    } else {
        (s.strip_suffix('s')?, 1.0)
    };
    let v: f64 = num.parse().ok()?;
    (v >= 0.0).then(|| Duration::from_secs_f64(v * mul))
}

/// One stage's numbers in a report built from the log.
#[derive(Debug, Clone, PartialEq)]
pub struct StageReport {
    pub stage: String,
    pub n: usize,
    pub p50: Duration,
    pub p95: Duration,
    pub max: Duration,
    pub budget: Option<Duration>,
    /// `slow` WARNs seen for it in the window.
    pub slow_warnings: usize,
}

impl StageReport {
    pub fn over_budget(&self) -> bool {
        self.budget.is_some_and(|b| self.p95 > b)
    }
}

/// Builds a per-stage report from debug.log lines with a timestamp at or after
/// `since` (an ISO prefix compare; `None` = everything). Reads the `[perf]`
/// summaries (p50 = the median of their p50s, p95/max = the worst seen) plus
/// the timing lines older code already writes, so it works on logs from
/// before this module too: `git refresh for N worktree(s) in`, hook
/// `handled in`, and `<source> refresh: … in`.
pub fn report_from_log<'a>(lines: impl IntoIterator<Item = &'a str>, since: Option<&str>) -> Vec<StageReport> {
    use std::collections::BTreeMap;
    #[derive(Default)]
    struct Acc {
        n: usize,
        p50s: Vec<Duration>,
        p95: Duration,
        max: Duration,
        raw: Vec<Duration>,
        slow: usize,
    }
    let mut acc: BTreeMap<String, Acc> = BTreeMap::new();
    for line in lines {
        let Some(rest) = line.strip_prefix('[') else { continue };
        let Some((ts, rest)) = rest.split_once("] ") else { continue };
        if since.is_some_and(|s| ts < s) {
            continue;
        }
        let Some(rest) = rest.strip_prefix('[') else { continue };
        let Some((level, rest)) = rest.split_once("] [") else { continue };
        let Some((module, msg)) = rest.split_once("] ") else { continue };
        match module {
            MODULE if level == "WARN" => {
                if let Some((stage, _)) = msg.split_once(" slow: ") {
                    acc.entry(stage.to_string()).or_default().slow += 1;
                }
            }
            MODULE => {
                for part in msg.split("; ") {
                    let mut words = part.split_whitespace();
                    let Some(stage) = words.next() else { continue };
                    let mut got = (None, None, None, None);
                    for w in words {
                        match w.split_once('=') {
                            Some(("n", v)) => got.0 = v.parse::<usize>().ok(),
                            Some(("p50", v)) => got.1 = parse_duration(v),
                            Some(("p95", v)) => got.2 = parse_duration(v),
                            Some(("max", v)) => got.3 = parse_duration(v),
                            _ => {}
                        }
                    }
                    if let (Some(n), Some(p50), Some(p95), Some(max)) = got {
                        let a = acc.entry(stage.to_string()).or_default();
                        a.n += n;
                        a.p50s.push(p50);
                        a.p95 = a.p95.max(p95);
                        a.max = a.max.max(max);
                    }
                }
            }
            _ => {
                let legacy = if module == "tui" && msg.starts_with("git refresh for ") {
                    Some("collector.git")
                } else if module == "hook-event" && msg.contains(" handled in ") {
                    Some("hook.event")
                } else if module == "integrations" {
                    ["github", "linear", "review requests"]
                        .iter()
                        .zip(["integrations.github", "integrations.linear", "integrations.reviews"])
                        .find(|(src, _)| msg.starts_with(&format!("{src} refresh: ")))
                        .map(|(_, stage)| stage)
                } else {
                    None
                };
                if let Some(stage) = legacy
                    && let Some(d) = msg.rsplit_once(" in ").and_then(|(_, t)| parse_duration(t))
                {
                    acc.entry(stage.to_string()).or_default().raw.push(d);
                }
            }
        }
    }
    acc.into_iter()
        .filter_map(|(stage, a)| {
            let budget = budget(&stage);
            // Raw samples win: they are exact. Otherwise fold the summaries.
            if let Some(sum) = summarize(a.raw.iter().copied()) {
                return Some(StageReport { stage, n: sum.n, p50: sum.p50, p95: sum.p95, max: sum.max, budget, slow_warnings: a.slow });
            }
            let p50 = summarize(a.p50s.iter().copied()).map(|s| s.p50);
            match p50 {
                Some(p50) => Some(StageReport { stage, n: a.n, p50, p95: a.p95, max: a.max, budget, slow_warnings: a.slow }),
                None if a.slow > 0 => Some(StageReport {
                    stage,
                    n: 0,
                    p50: Duration::ZERO,
                    p95: Duration::ZERO,
                    max: Duration::ZERO,
                    budget,
                    slow_warnings: a.slow,
                }),
                None => None,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }

    #[test]
    fn summarize_uses_nearest_rank() {
        let s = summarize((1..=100).map(d)).unwrap();
        assert_eq!((s.n, s.p50, s.p95, s.max), (100, d(50), d(95), d(100)));
        assert_eq!(summarize([d(7)]).unwrap().p95, d(7));
        assert!(summarize(std::iter::empty()).is_none());
    }

    #[test]
    fn a_stage_turns_slow_and_recovers_on_p95() {
        let mut p = Perf::new();
        let now = Instant::now();
        for _ in 0..30 {
            p.record_at("ui.frame", d(2), now);
        }
        // One outlier among 31 samples is not a slow stage.
        p.record_at("ui.frame", d(400), now);
        assert!(p.slow_stages().is_empty());
        for _ in 0..10 {
            p.record_at("ui.frame", d(40), now);
        }
        assert_eq!(p.slow_stages(), vec!["ui.frame"]);
        for _ in 0..WINDOW {
            p.record_at("ui.frame", d(2), now);
        }
        assert!(p.slow_stages().is_empty());
    }

    #[test]
    fn a_stage_is_not_judged_on_too_few_samples() {
        let mut p = Perf::new();
        let now = Instant::now();
        p.record_at("collector.cycle", d(5_000), now);
        assert!(p.slow_stages().is_empty());
    }

    #[test]
    fn stages_without_a_budget_are_never_slow() {
        let mut p = Perf::new();
        let now = Instant::now();
        for _ in 0..30 {
            p.record_at("something.else", d(60_000), now);
        }
        assert!(p.slow_stages().is_empty());
    }

    #[test]
    fn usage_reports_cover_every_sample_in_the_hour() {
        let mut p = Perf::new();
        let t0 = Instant::now();
        // As if reports were on (is_on is false in tests): fill the hour directly.
        for i in 1..=100 {
            p.record_at("collector.git", d(i), t0);
            let s = p.stages.iter_mut().find(|s| s.name == "collector.git").unwrap();
            s.hour.push(d(i));
        }
        p.record_at("not.budgeted", d(5), t0);
        let events = p.usage_events(1);
        assert_eq!(events.len(), 1, "budgeted stages only");
        let a = &events[0].attrs;
        use crate::telemetry::schema::Attr::Int;
        assert_eq!((a["n"].clone(), a["p95_us"].clone(), a["max_us"].clone()), (Int(100), Int(95_000), Int(100_000)));
        // The 5-minute summary line doesn't reset the hour…
        p.record_at("collector.git", d(1), t0 + REPORT_EVERY);
        assert_eq!(p.usage_events(1).len(), 1);
        // …the hourly report does.
        p.report_usage(t0 + USAGE_REPORT_EVERY);
        assert!(p.usage_events(1).is_empty());
    }

    #[test]
    fn report_starts_a_new_period() {
        let mut p = Perf::new();
        let now = Instant::now();
        p.record_at("ui.frame", d(3), now);
        assert_eq!(p.summary_line().unwrap(), "ui.frame n=1 p50=3.00ms p95=3.00ms max=3.00ms budget=16.00ms");
        p.report(now);
        assert_eq!(p.summary_line(), None);
    }

    #[test]
    fn parse_duration_reads_debug_and_ms_formats() {
        assert_eq!(parse_duration("8.1s"), Some(Duration::from_secs_f64(8.1)));
        assert_eq!(parse_duration("12.5ms"), Some(Duration::from_secs_f64(0.0125)));
        assert_eq!(parse_duration("830µs"), Some(Duration::from_secs_f64(830e-6)));
        assert_eq!(parse_duration("40ns"), Some(Duration::from_nanos(40)));
        assert_eq!(parse_duration("fast"), None);
    }

    #[test]
    fn report_from_log_reads_summaries_warnings_and_legacy_lines() {
        let log = "\
[2026-10-27T00:00:00.000Z] [INFO] [perf] ui.frame n=10 p50=1.00ms p95=4.00ms max=9.00ms budget=16.00ms; collector.cycle n=5 p50=20.00ms p95=900.00ms max=1.20s budget=500.00ms
[2026-10-27T00:05:00.000Z] [INFO] [perf] ui.frame n=20 p50=3.00ms p95=6.00ms max=7.00ms budget=16.00ms
[2026-10-27T00:05:01.000Z] [WARN] [perf] collector.cycle slow: p95 900.00ms > budget 500.00ms (n=5, max 1.20s)
[2026-10-27T00:06:00.000Z] [DEBUG] [tui] git refresh for 56 worktree(s) in 8.1s
[2026-10-27T00:06:30.000Z] [DEBUG] [tui] git refresh for 1 worktree(s) in 28.5ms
[2026-10-27T00:07:00.000Z] [DEBUG] [hook-event] event=Stop handled in 1.88ms
[2026-10-27T00:08:00.000Z] [INFO] [integrations] review requests refresh: Ran { fetched: 3, failed: 0, aborted: false } in 6.3s
[2026-10-27T00:09:00.000Z] [INFO] [sync] unrelated line in 3s";
        let r = report_from_log(log.lines(), None);
        let get = |s: &str| r.iter().find(|x| x.stage == s).unwrap_or_else(|| panic!("{s} in {r:?}"));
        let f = get("ui.frame");
        assert_eq!((f.n, f.p95, f.max), (30, d(6), d(9)));
        assert!(!f.over_budget());
        let c = get("collector.cycle");
        assert!(c.over_budget());
        assert_eq!(c.slow_warnings, 1);
        let g = get("collector.git");
        assert_eq!((g.n, g.max), (2, Duration::from_secs_f64(8.1)));
        assert!(g.over_budget());
        assert_eq!(get("hook.event").n, 1);
        assert_eq!(get("integrations.reviews").p50, Duration::from_secs_f64(6.3));
        assert_eq!(r.len(), 5);

        let later = report_from_log(log.lines(), Some("2026-10-27T00:06"));
        assert!(later.iter().all(|x| x.stage != "ui.frame"));
    }
}
