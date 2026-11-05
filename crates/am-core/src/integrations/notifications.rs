//! GitHub's notifications inbox as a free "something changed" signal.
//!
//! `GET /notifications?all=true` with `If-Modified-Since` answers `304` when
//! nothing is new, and a 304 costs no rate limit. GitHub asks for at least
//! `X-Poll-Interval` (60 s) between polls, which is still well under the
//! default PR (180 s) and review (120 s) intervals. Each thread names its repo
//! and, for a PR, its number: review requests, reviews, comments, merges and
//! closes on PRs you take part in, and failed Actions runs you started
//! (`CheckSuite`, no PR number). It doesn't cover CI going green or merge
//! conflicts; [`super::ci_watch`] covers CI.
//!
//! The first answer only records where the inbox stands, so starting am
//! doesn't refetch every PR with an old notification.

use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::github::GhRunner;
use super::rest::rest_get;
use crate::debug;

/// GitHub's usual `X-Poll-Interval`; never poll faster even if it says less.
pub const MIN_INTERVAL: Duration = Duration::from_secs(60);
/// After a failed or refused poll (no `notifications` scope, offline…).
pub const RETRY_INTERVAL: Duration = Duration::from_secs(300);
const PATH: &str = "notifications?all=true&per_page=50";

/// One notification thread, reduced to what the watchers need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thread {
    /// `review_requested`, `author`, `comment`, `ci_activity`, `state_change`…
    pub reason: String,
    /// `subject.type`: `PullRequest`, `CheckSuite`, `Issue`…
    pub kind: String,
    /// `owner/name`, lower-cased.
    pub repo: String,
    /// The PR number when the subject is a pull request.
    pub pr_number: Option<u64>,
    /// ISO-8601 UTC (`2026-11-05T23:17:23Z`), so it compares as a string.
    pub updated_at: String,
}

/// `…/repos/o/r/pulls/123` → 123.
fn pr_number(url: &str) -> Option<u64> {
    let (head, n) = url.rsplit_once('/')?;
    head.ends_with("/pulls").then(|| n.parse().ok()).flatten()
}

/// The threads in one `/notifications` body. `None` when it isn't a list.
pub fn parse_threads(body: &str) -> Option<Vec<Thread>> {
    let json: Value = serde_json::from_str(body.trim()).ok()?;
    let s = |v: &Value, p: &str| v.pointer(p).and_then(Value::as_str).unwrap_or("").to_string();
    Some(
        json.as_array()?
            .iter()
            .map(|t| {
                let kind = s(t, "/subject/type");
                let pr_number = (kind == "PullRequest").then(|| pr_number(&s(t, "/subject/url"))).flatten();
                Thread {
                    reason: s(t, "/reason"),
                    kind,
                    repo: s(t, "/repository/full_name").to_lowercase(),
                    pr_number,
                    updated_at: s(t, "/updated_at"),
                }
            })
            .collect(),
    )
}

/// Polls the inbox no faster than GitHub allows and hands back what's new.
#[derive(Debug, Default)]
pub struct NotificationsWatch {
    last_modified: Option<String>,
    /// Newest `updated_at` seen; `None` until the first answer.
    seen_until: Option<String>,
    next_at: Option<Instant>,
    /// For logging the first failure only.
    failing: bool,
}

impl NotificationsWatch {
    pub fn due(&self, now: Instant) -> bool {
        self.next_at.is_none_or(|t| now >= t)
    }

    /// Ask GitHub (when [`due`](Self::due)) and return the threads updated
    /// since the last answer: none for a 304, the first answer or a failure.
    pub fn poll(&mut self, gh: &dyn GhRunner, cwd: &Path, now: Instant) -> Vec<Thread> {
        let lm = self.last_modified.clone();
        let headers: Vec<(&str, &str)> = lm.as_deref().map(|v| ("If-Modified-Since", v)).into_iter().collect();
        let r = match rest_get(gh, cwd, PATH, &headers) {
            Ok(r) => r,
            Err(e) => return self.failed(now, &e),
        };
        if r.not_modified() {
            self.next_at = Some(now + r.poll_interval().unwrap_or(MIN_INTERVAL).max(MIN_INTERVAL));
            return Vec::new();
        }
        if r.status != 200 {
            return self.failed(now, &format!("HTTP {}", r.status));
        }
        let Some(threads) = parse_threads(&r.body) else {
            return self.failed(now, "unparseable notifications body");
        };
        self.next_at = Some(now + r.poll_interval().unwrap_or(MIN_INTERVAL).max(MIN_INTERVAL));
        if self.failing {
            debug!("github", "notifications watch recovered");
            self.failing = false;
        }
        self.last_modified = r.header("last-modified").map(str::to_string);
        let newest = threads.iter().map(|t| t.updated_at.as_str()).max().map(str::to_string);
        let fresh: Vec<Thread> = match &self.seen_until {
            None => Vec::new(),
            Some(seen) => threads.into_iter().filter(|t| t.updated_at > *seen).collect(),
        };
        if let Some(n) = newest
            && self.seen_until.as_ref().is_none_or(|s| n > *s)
        {
            self.seen_until = Some(n);
        } else if self.seen_until.is_none() {
            // An empty inbox: anything that turns up later is new.
            self.seen_until = Some(String::new());
        }
        debug!("github", "notifications: {} new thread(s)", fresh.len());
        fresh
    }

    fn failed(&mut self, now: Instant, e: &str) -> Vec<Thread> {
        if !self.failing {
            debug!("github", "notifications watch failed, retrying in {}s: {e}", RETRY_INTERVAL.as_secs());
            self.failing = true;
        }
        self.next_at = Some(now + RETRY_INTERVAL);
        Vec::new()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::super::github::GhOutput;
    use super::super::github::tests::FakeGh;
    use super::super::rest::tests::include;
    use super::*;
    use serde_json::json;

    pub fn thread(reason: &str, kind: &str, repo: &str, url: Option<&str>, updated: &str) -> Value {
        json!({
            "reason": reason, "updated_at": updated,
            "subject": {"type": kind, "url": url, "title": "t"},
            "repository": {"full_name": repo}
        })
    }

    pub fn page(lm: &str, threads: Vec<Value>) -> GhOutput {
        GhOutput::success(include(
            200,
            &[("Last-Modified", lm), ("X-Poll-Interval", "60")],
            &Value::Array(threads).to_string(),
        ))
    }

    pub fn not_modified() -> GhOutput {
        GhOutput { ok: false, stdout: include(304, &[("X-Poll-Interval", "60")], ""), stderr: "gh: HTTP 304".into() }
    }

    #[test]
    fn parses_pr_and_check_suite_threads() {
        let body = Value::Array(vec![
            thread("review_requested", "PullRequest", "Acme/Web", Some("https://api.github.com/repos/Acme/Web/pulls/42"), "2026-11-05T10:00:00Z"),
            thread("ci_activity", "CheckSuite", "acme/web", None, "2026-11-05T09:00:00Z"),
        ])
        .to_string();
        let t = parse_threads(&body).unwrap();
        assert_eq!(t[0].repo, "acme/web");
        assert_eq!(t[0].pr_number, Some(42));
        assert_eq!(t[1].kind, "CheckSuite");
        assert_eq!(t[1].pr_number, None);
        assert!(parse_threads("{}").is_none());
    }

    #[test]
    fn first_answer_primes_then_only_newer_threads_come_back() {
        let old = thread("author", "PullRequest", "a/b", Some("https://api.github.com/repos/a/b/pulls/1"), "2026-11-05T10:00:00Z");
        let new = thread("comment", "PullRequest", "a/b", Some("https://api.github.com/repos/a/b/pulls/2"), "2026-11-05T11:00:00Z");
        let gh = FakeGh::new(vec![page("LM1", vec![old.clone()]), not_modified(), page("LM2", vec![new, old])]);
        let mut w = NotificationsWatch::default();
        let t0 = Instant::now();
        assert!(w.poll(&gh, Path::new("/"), t0).is_empty(), "the first answer nudges nothing");
        assert!(!w.due(t0 + Duration::from_secs(59)));
        assert!(w.due(t0 + MIN_INTERVAL));
        assert!(w.poll(&gh, Path::new("/"), t0 + MIN_INTERVAL).is_empty());
        let fresh = w.poll(&gh, Path::new("/"), t0 + MIN_INTERVAL * 2);
        assert_eq!(fresh.iter().map(|t| t.pr_number).collect::<Vec<_>>(), [Some(2)]);
        let calls = gh.calls.lock().unwrap();
        assert!(!calls[0].iter().any(|a| a.starts_with("If-Modified-Since")));
        assert!(calls[1].contains(&"If-Modified-Since: LM1".to_string()));
        assert!(calls[2].contains(&"If-Modified-Since: LM1".to_string()), "a 304 keeps the old Last-Modified");
    }

    #[test]
    fn an_empty_inbox_then_a_thread_is_new() {
        let t = thread("review_requested", "PullRequest", "a/b", Some("https://api.github.com/repos/a/b/pulls/7"), "2026-11-05T11:00:00Z");
        let gh = FakeGh::new(vec![page("LM1", vec![]), page("LM2", vec![t])]);
        let mut w = NotificationsWatch::default();
        let t0 = Instant::now();
        assert!(w.poll(&gh, Path::new("/"), t0).is_empty());
        assert_eq!(w.poll(&gh, Path::new("/"), t0 + MIN_INTERVAL).len(), 1);
    }

    #[test]
    fn failures_back_off() {
        let gh = FakeGh::new(vec![GhOutput::failure("HTTP 403: Resource not accessible")]);
        let mut w = NotificationsWatch::default();
        let t0 = Instant::now();
        assert!(w.poll(&gh, Path::new("/"), t0).is_empty());
        assert!(!w.due(t0 + MIN_INTERVAL));
        assert!(w.due(t0 + RETRY_INTERVAL));
    }
}
