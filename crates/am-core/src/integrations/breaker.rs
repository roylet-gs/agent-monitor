//! Per-source circuit breaker for the GitHub / Linear pollers.
//!
//! Real logs from the TS app showed a 9-hour offline stretch producing 2,483
//! Linear WARNs (one per branch per 180 s cycle, no backoff) plus multi-line
//! `gh` stderr dumps. The breaker fixes both:
//!
//! - A **network-class** failure (DNS, connection refused, "error connecting",
//!   timeouts, 5xx/429 "service unavailable") opens the circuit: the caller
//!   aborts the rest of that source's cycle, and later cycles are skipped until
//!   an exponential backoff (30 s doubling, capped at 15 min) expires.
//!   Other failures (auth, bad query, a repo that isn't on GitHub) mark the
//!   source failing for [`IntegrationHealth`](super::IntegrationHealth) but do
//!   not back off — retrying them is cheap and they aren't load on a dead link.
//! - Logging is level-shifted: WARN only on the healthy→failing transition,
//!   INFO on failing→recovered, DEBUG for everything in between.
//!
//! A user-initiated refresh passes `force`, which bypasses the backoff (the TS
//! `clearPrBackoff` rule): pressing `r` after reconnecting must not be
//! silently swallowed.
//!
//! Time is passed in explicitly (`now: Instant`) so the rules are unit-testable.

use std::time::{Duration, Instant};

use crate::{debug, info, warn};

pub const BACKOFF_INITIAL: Duration = Duration::from_secs(30);
pub const BACKOFF_MAX: Duration = Duration::from_secs(15 * 60);

#[derive(Debug, Clone)]
pub struct Breaker {
    source: &'static str,
    failing: bool,
    /// Consecutive network-class failures; drives the backoff exponent.
    network_failures: u32,
    open_until: Option<Instant>,
    last_error: Option<String>,
}

impl Breaker {
    /// `source` is used as the log module, e.g. `"github"` / `"linear"`.
    pub fn new(source: &'static str) -> Self {
        Breaker { source, failing: false, network_failures: 0, open_until: None, last_error: None }
    }

    /// Whether a cycle may run now. `force` (a user refresh) always may.
    pub fn allow(&self, now: Instant, force: bool) -> bool {
        match self.open_until {
            Some(until) if now < until && !force => {
                debug!(
                    self.source,
                    "skipping {} refresh: circuit open for another {}s",
                    self.source,
                    (until - now).as_secs()
                );
                false
            }
            Some(_) if force => {
                debug!(self.source, "user refresh bypasses {} backoff", self.source);
                true
            }
            _ => true,
        }
    }

    /// Record a cycle in which every fetch that ran succeeded.
    pub fn on_success(&mut self) {
        if self.failing {
            info!(
                self.source,
                "{} recovered after {} network failure(s); last error was: {}",
                self.source,
                self.network_failures,
                self.last_error.as_deref().unwrap_or("-")
            );
        }
        self.failing = false;
        self.network_failures = 0;
        self.open_until = None;
        self.last_error = None;
    }

    /// Record a cycle that failed. `network` opens the circuit with backoff.
    pub fn on_failure(&mut self, now: Instant, error: &str, network: bool) {
        let error = summarize_error(error);
        let backoff = if network {
            self.network_failures = self.network_failures.saturating_add(1);
            let b = backoff_for(self.network_failures);
            self.open_until = Some(now + b);
            Some(b)
        } else {
            None
        };
        let suffix = match backoff {
            Some(b) => format!("; backing off {}s", b.as_secs()),
            None => String::new(),
        };
        if self.failing {
            debug!(self.source, "{} still failing: {error}{suffix}", self.source);
        } else {
            warn!(self.source, "{} failing: {error}{suffix}", self.source);
        }
        self.failing = true;
        self.last_error = Some(error);
    }

    pub fn is_failing(&self) -> bool {
        self.failing
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// Remaining backoff, if the circuit is open at `now`.
    pub fn open_for(&self, now: Instant) -> Option<Duration> {
        self.open_until.filter(|u| *u > now).map(|u| u - now)
    }
}

/// 30 s, 60 s, 120 s, … capped at 15 min.
pub fn backoff_for(consecutive: u32) -> Duration {
    let shift = consecutive.saturating_sub(1).min(16);
    BACKOFF_INITIAL.saturating_mul(1u32 << shift).min(BACKOFF_MAX)
}

/// Does this error mean "the network/service is unreachable" (back off) rather
/// than "the request was answered with a problem" (don't)? Works on gh stderr,
/// ureq error text and our own synthetic messages.
pub fn is_network_error(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    const NEEDLES: &[&str] = &[
        "error connecting",
        "enotfound",
        "eai_again",
        "no such host",
        "host not found",
        "hostnotfound",
        "temporary failure in name resolution",
        "nodename nor servname",
        "dns",
        "connection refused",
        "econnrefused",
        "connection reset",
        "econnreset",
        "connection failed",
        "connectionfailed",
        "network is unreachable",
        "network is down",
        "no route to host",
        "timed out",
        "timeout",
        "tls handshake",
        "http 502",
        "http 503",
        "http 504",
    ];
    NEEDLES.iter().any(|n| m.contains(n))
}

/// One short line for logs and the health hint: first non-empty line, capped.
/// gh's stderr is often several lines ("error connecting…\ncheck your internet…").
pub fn summarize_error(msg: &str) -> String {
    let line = msg.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("unknown error");
    let line = line.strip_prefix("gh: ").unwrap_or(line);
    if line.chars().count() > 200 {
        let mut s: String = line.chars().take(200).collect();
        s.push('…');
        s
    } else {
        line.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_and_caps_at_fifteen_minutes() {
        assert_eq!(backoff_for(1), Duration::from_secs(30));
        assert_eq!(backoff_for(2), Duration::from_secs(60));
        assert_eq!(backoff_for(3), Duration::from_secs(120));
        assert_eq!(backoff_for(5), Duration::from_secs(480));
        assert_eq!(backoff_for(6), BACKOFF_MAX);
        assert_eq!(backoff_for(1000), BACKOFF_MAX);
    }

    #[test]
    fn network_failure_opens_the_circuit_until_backoff_expires() {
        let t0 = Instant::now();
        let mut b = Breaker::new("test");
        assert!(b.allow(t0, false));
        b.on_failure(t0, "error connecting to api.github.com\ncheck your internet", true);
        assert!(b.is_failing());
        assert_eq!(b.last_error(), Some("error connecting to api.github.com"));
        assert!(!b.allow(t0 + Duration::from_secs(10), false));
        assert!(b.allow(t0 + Duration::from_secs(31), false));

        // Second consecutive network failure doubles the wait.
        let t1 = t0 + Duration::from_secs(31);
        b.on_failure(t1, "dial tcp: lookup api.github.com: no such host", true);
        assert!(!b.allow(t1 + Duration::from_secs(59), false));
        assert!(b.allow(t1 + Duration::from_secs(61), false));
    }

    #[test]
    fn force_bypasses_the_backoff() {
        let t0 = Instant::now();
        let mut b = Breaker::new("test");
        b.on_failure(t0, "connection refused", true);
        assert!(!b.allow(t0, false));
        assert!(b.allow(t0, true));
    }

    #[test]
    fn non_network_failures_mark_failing_without_backing_off() {
        let t0 = Instant::now();
        let mut b = Breaker::new("test");
        b.on_failure(t0, "HTTP 401: Bad credentials", false);
        assert!(b.is_failing());
        assert!(b.allow(t0, false));
        assert_eq!(b.open_for(t0), None);
    }

    #[test]
    fn success_resets_everything() {
        let t0 = Instant::now();
        let mut b = Breaker::new("test");
        b.on_failure(t0, "timed out", true);
        b.on_failure(t0, "timed out", true);
        b.on_success();
        assert!(!b.is_failing());
        assert_eq!(b.last_error(), None);
        assert!(b.allow(t0, false));
        b.on_failure(t0, "timed out", true);
        assert_eq!(b.open_for(t0), Some(BACKOFF_INITIAL));
    }

    #[test]
    fn classifies_network_errors() {
        for msg in [
            "error connecting to api.github.com",
            "getaddrinfo ENOTFOUND api.linear.app",
            "connect ECONNREFUSED 127.0.0.1:443",
            "Post \"https://api.github.com/graphql\": dial tcp: lookup api.github.com: no such host",
            "gh timed out after 15s",
            "io: Connection refused (os error 61)",
            "HTTP 503: Service Unavailable",
        ] {
            assert!(is_network_error(msg), "{msg}");
        }
        for msg in [
            "no pull requests found for branch \"x\"",
            "GraphQL: Could not resolve to a Repository with the name 'a/b'.",
            "HTTP 401: Bad credentials",
            "none of the git remotes configured for this repository point to a known GitHub host",
        ] {
            assert!(!is_network_error(msg), "{msg}");
        }
    }

    #[test]
    fn summarize_keeps_one_short_line() {
        assert_eq!(summarize_error("\n  gh: boom\nmore\n"), "boom");
        assert_eq!(summarize_error(""), "unknown error");
        assert_eq!(summarize_error(&"x".repeat(500)).chars().count(), 201);
    }
}
