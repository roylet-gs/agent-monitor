//! Free CI change detection for open PRs: conditional REST on check suites.
//!
//! For each watched branch, `GET repos/{owner}/{repo}/commits/<branch>/check-suites`
//! with `If-None-Match` answers `304` (no rate-limit cost) until a suite
//! changes. Asking by **branch**, not sha, means a push shows up too: the new
//! commit's suites replace the old ones. Suites rather than check runs because
//! a monorepo PR can have hundreds of runs (one had 335 runs in 11 suites),
//! which would take several pages per poll. Suites with no runs are left out:
//! every installed app with the Checks permission leaves a `queued` suite with
//! zero runs on every commit, forever. Commit statuses (`…/status`) are asked
//! on a branch's first look and afterwards only if it had any.
//!
//! #169 tried something similar and was reverted for burning the rate limit.
//! What's different here:
//! - **A GraphQL refetch only on a change of bucket** (pending / failing /
//!   passing / none), worked out locally from the REST body. A 200 that leaves
//!   the bucket alone costs one REST request and nothing else, and REST
//!   (`core`) and GraphQL are separate rate-limit buckets.
//! - **At most [`CI_WATCH_MAX`] branches**, open PRs only, pending ones first.
//! - **An hourly budget** of [`HOURLY_CAP`] answers that weren't 304: polling
//!   slows to [`SLOW_INTERVAL`] at half of it and stops at all of it until the
//!   hour rolls on, and it stops while `X-RateLimit-Remaining` is under
//!   [`MIN_REMAINING`].

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::Value;

use super::github::{Check, GhRunner, derive_checks_status};
use super::rest::{RestResponse, encode_segment, rest_get};
use crate::model::ChecksStatus;
use crate::{debug, info};

pub const CI_WATCH_INTERVAL: Duration = Duration::from_secs(15);
pub const SLOW_INTERVAL: Duration = Duration::from_secs(60);
pub const CI_WATCH_MAX: usize = 8;
/// Non-304 REST answers allowed per rolling hour (out of the 5000 `core`).
pub const HOURLY_CAP: usize = 300;
/// Stop while the `core` bucket has fewer requests left than this.
pub const MIN_REMAINING: u64 = 1000;
const HOUR: Duration = Duration::from_secs(3600);

/// A branch with an open PR to keep an eye on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub repo_id: String,
    pub repo_path: PathBuf,
    pub branch: String,
    /// What the last GraphQL fetch said.
    pub cached: ChecksStatus,
}

#[derive(Debug, Default)]
struct Watched {
    suites_etag: Option<String>,
    status_etag: Option<String>,
    suites: Option<ChecksStatus>,
    statuses: Option<ChecksStatus>,
    /// The branch had commit statuses at its last look.
    has_statuses: bool,
    /// The combined bucket at the last poll; `None` before the first.
    bucket: Option<ChecksStatus>,
}

#[derive(Debug, Default)]
pub struct CiWatch {
    watched: HashMap<(String, String), Watched>,
    next_at: Option<Instant>,
    /// When each non-304 answer of the last hour came.
    spent: VecDeque<Instant>,
    paused_until: Option<Instant>,
}

/// Suites with runs → the same buckets as the GraphQL rollup.
pub fn suites_bucket(body: &str) -> Option<ChecksStatus> {
    let json: Value = serde_json::from_str(body.trim()).ok()?;
    let checks: Vec<Check> = json
        .get("check_suites")?
        .as_array()?
        .iter()
        .filter(|s| s.get("latest_check_runs_count").and_then(Value::as_u64).unwrap_or(0) > 0)
        .map(|s| Check {
            status: s.get("status").and_then(Value::as_str).unwrap_or("").to_ascii_uppercase(),
            conclusion: s.get("conclusion").and_then(Value::as_str).unwrap_or("").to_ascii_uppercase(),
            ..Default::default()
        })
        .collect();
    Some(derive_checks_status(&checks))
}

/// A combined-status body → its bucket and whether it had any statuses.
pub fn statuses_bucket(body: &str) -> Option<(ChecksStatus, bool)> {
    let json: Value = serde_json::from_str(body.trim()).ok()?;
    let checks: Vec<Check> = json
        .get("statuses")?
        .as_array()?
        .iter()
        .map(|s| {
            let state = s.get("state").and_then(Value::as_str).unwrap_or("").to_ascii_uppercase();
            let done = matches!(state.as_str(), "SUCCESS" | "FAILURE" | "ERROR");
            Check {
                status: if done { "COMPLETED".into() } else { "PENDING".into() },
                conclusion: if done { state } else { String::new() },
                ..Default::default()
            }
        })
        .collect();
    let any = !checks.is_empty();
    Some((derive_checks_status(&checks), any))
}

/// Failing beats pending beats passing beats none.
fn combine(a: ChecksStatus, b: ChecksStatus) -> ChecksStatus {
    use ChecksStatus::*;
    match (a, b) {
        (Failing, _) | (_, Failing) => Failing,
        (Pending, _) | (_, Pending) => Pending,
        (Passing, _) | (_, Passing) => Passing,
        _ => None,
    }
}

/// Seconds until the `x-ratelimit-reset` epoch, capped at an hour.
fn until_reset(r: &RestResponse) -> Duration {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    r.header("x-ratelimit-reset")
        .and_then(|v| v.parse::<u64>().ok())
        .map(|reset| Duration::from_secs(reset.saturating_sub(now)))
        .unwrap_or(Duration::from_secs(600))
        .clamp(Duration::from_secs(60), HOUR)
}

/// One request of a poll: which target, what, and the ETag to send.
struct Req {
    idx: usize,
    path: String,
    etag: Option<String>,
}

/// Run every request at once (at most 2 × [`CI_WATCH_MAX`] gh processes,
/// like the PR fetch's parallel chunks): one by one, 8 branches took ~10 s.
fn fetch_all(gh: &dyn GhRunner, targets: &[Target], reqs: &[Req]) -> Vec<Result<RestResponse, String>> {
    let one = |r: &Req| {
        let headers: Vec<(&str, &str)> = r.etag.as_deref().map(|e| ("If-None-Match", e)).into_iter().collect();
        rest_get(gh, &targets[r.idx].repo_path, &r.path, &headers)
    };
    if reqs.len() <= 1 {
        return reqs.iter().map(one).collect();
    }
    std::thread::scope(|scope| {
        let handles: Vec<_> = reqs.iter().map(|r| scope.spawn(move || one(r))).collect();
        handles.into_iter().map(|h| h.join().unwrap_or_else(|_| Err("CI watch request panicked".into()))).collect()
    })
}

impl CiWatch {
    pub fn due(&self, now: Instant) -> bool {
        self.next_at.is_none_or(|t| now >= t)
    }

    /// Non-304 answers in the last hour.
    pub fn spent(&self) -> usize {
        self.spent.len()
    }

    /// Poll every target (at most [`CI_WATCH_MAX`]) and return the branches
    /// whose CI bucket moved: those are worth a GraphQL refetch.
    pub fn poll(&mut self, gh: &dyn GhRunner, targets: &[Target], now: Instant) -> Vec<(String, String)> {
        while self.spent.front().is_some_and(|t| now.duration_since(*t) >= HOUR) {
            self.spent.pop_front();
        }
        if let Some(until) = self.paused_until {
            if now < until {
                self.next_at = Some(until);
                return Vec::new();
            }
            info!("github", "CI watch resumed");
            self.paused_until = None;
        }
        if self.spent.len() >= HOURLY_CAP {
            self.pause_for_budget(now);
            return Vec::new();
        }
        let room = HOURLY_CAP - self.spent.len();
        let targets = &targets[..targets.len().min(CI_WATCH_MAX).min(room)];
        self.watched.retain(|k, _| targets.iter().any(|t| (&t.repo_id, &t.branch) == (&k.0, &k.1)));
        let key = |t: &Target| (t.repo_id.clone(), t.branch.clone());
        let first: Vec<bool> =
            targets.iter().map(|t| self.watched.get(&key(t)).is_none_or(|w| w.bucket.is_none())).collect();
        let nudges = self.poll_targets(gh, targets, &first, now);
        if self.paused_until.is_none() {
            let slow = self.spent.len() * 2 >= HOURLY_CAP;
            let next = now + if slow { SLOW_INTERVAL } else { CI_WATCH_INTERVAL };
            // A failure may already have pushed it further out.
            self.later(next);
        }
        nudges
    }

    fn poll_targets(&mut self, gh: &dyn GhRunner, targets: &[Target], first: &[bool], now: Instant) -> Vec<(String, String)> {
        let key = |t: &Target| (t.repo_id.clone(), t.branch.clone());
        let mut answered = vec![false; targets.len()];

        // Wave 1: every target's check suites.
        let reqs: Vec<Req> = targets
            .iter()
            .enumerate()
            .map(|(idx, t)| Req {
                idx,
                path: format!("repos/{{owner}}/{{repo}}/commits/{}/check-suites?per_page=100", encode_segment(&t.branch)),
                etag: self.watched.get(&key(t)).and_then(|w| w.suites_etag.clone()),
            })
            .collect();
        let mut stop = false;
        for (req, answer) in reqs.iter().zip(fetch_all(gh, targets, &reqs)) {
            let t = &targets[req.idx];
            let Some(r) = self.take(answer, t, now, &mut stop) else { continue };
            answered[req.idx] = true;
            let w = self.watched.entry(key(t)).or_default();
            if r.status == 200 {
                w.suites_etag = r.header("etag").map(str::to_string);
                w.suites = suites_bucket(&r.body);
            } else if !r.not_modified() {
                // 404/422: the branch isn't on GitHub (yet).
                w.suites_etag = None;
                w.suites = Some(ChecksStatus::None);
            }
        }
        if stop {
            // ETags and suites are kept; the next poll judges the buckets.
            return Vec::new();
        }

        // Wave 2: commit statuses, on a first look or where there were some.
        let room = HOURLY_CAP.saturating_sub(self.spent.len());
        let reqs: Vec<Req> = targets
            .iter()
            .enumerate()
            .filter(|(i, t)| answered[*i] && (first[*i] || self.watched.get(&key(t)).is_some_and(|w| w.has_statuses)))
            .take(room)
            .map(|(idx, t)| Req {
                idx,
                path: format!("repos/{{owner}}/{{repo}}/commits/{}/status", encode_segment(&t.branch)),
                etag: self.watched.get(&key(t)).and_then(|w| w.status_etag.clone()),
            })
            .collect();
        for (req, answer) in reqs.iter().zip(fetch_all(gh, targets, &reqs)) {
            let t = &targets[req.idx];
            let Some(r) = self.take(answer, t, now, &mut stop) else { continue };
            let w = self.watched.entry(key(t)).or_default();
            if r.status == 200 {
                w.status_etag = r.header("etag").map(str::to_string);
                if let Some((bucket, any)) = statuses_bucket(&r.body) {
                    w.statuses = Some(bucket);
                    w.has_statuses = any;
                }
            }
        }
        if stop {
            return Vec::new();
        }

        let mut nudges = Vec::new();
        for t in targets.iter().zip(&answered).filter(|(_, a)| **a).map(|(t, _)| t) {
            let w = self.watched.entry(key(t)).or_default();
            let Some(suites) = w.suites else { continue };
            let bucket = combine(suites, w.statuses.unwrap_or(ChecksStatus::None));
            let moved = match w.bucket {
                // First look: only worth a refetch if GraphQL said otherwise.
                None => bucket != t.cached,
                Some(prev) => prev != bucket,
            };
            if moved {
                debug!("github", "CI watch: {} {:?} → {bucket:?}", t.branch, w.bucket.unwrap_or(t.cached));
                nudges.push(key(t));
            }
            w.bucket = Some(bucket);
        }
        nudges
    }

    /// Book one answer against the budget and the rate limit. `None`: no
    /// usable answer; `stop` is set when the poll should go no further.
    fn take(&mut self, answer: Result<RestResponse, String>, t: &Target, now: Instant, stop: &mut bool) -> Option<RestResponse> {
        let r = match answer {
            Ok(r) => r,
            Err(e) => {
                debug!("github", "CI watch: {} failed: {e}", t.branch);
                self.later(now + SLOW_INTERVAL);
                *stop = true;
                return None;
            }
        };
        if !r.not_modified() {
            self.spent.push_back(now);
        }
        if r.rate_remaining().is_some_and(|left| left < MIN_REMAINING) {
            if self.paused_until.is_none() {
                let wait = until_reset(&r);
                info!("github", "CI watch paused for {}s: GitHub rate limit low", wait.as_secs());
                self.paused_until = Some(now + wait);
                self.next_at = self.paused_until;
            }
            *stop = true;
            return None;
        }
        if r.status == 403 || r.status == 429 || r.status >= 500 {
            debug!("github", "CI watch: {} answered HTTP {}", t.branch, r.status);
            self.later(now + SLOW_INTERVAL);
            *stop = true;
            return None;
        }
        Some(r)
    }

    /// Wait at least until `at` (never shortens a pause).
    fn later(&mut self, at: Instant) {
        self.next_at = Some(self.next_at.filter(|t| *t > at).unwrap_or(at));
    }

    fn pause_for_budget(&mut self, now: Instant) {
        let until = self.spent.front().map_or(now + HOUR, |t| *t + HOUR);
        info!(
            "github",
            "CI watch paused for {}s: {HOURLY_CAP} changed answers in the last hour",
            until.saturating_duration_since(now).as_secs()
        );
        self.paused_until = Some(until);
        self.next_at = Some(until);
    }
}

#[cfg(test)]
mod tests {
    use super::super::github::GhOutput;
    use super::super::github::tests::FakeGh;
    use super::super::rest::tests::include;
    use super::*;
    use serde_json::json;

    fn suite(status: &str, conclusion: Option<&str>, runs: u64) -> Value {
        json!({"status": status, "conclusion": conclusion, "latest_check_runs_count": runs})
    }

    fn suites(etag: &str, list: Vec<Value>) -> GhOutput {
        GhOutput::success(include(
            200,
            &[("ETag", etag), ("X-RateLimit-Remaining", "4000")],
            &json!({"total_count": list.len(), "check_suites": list}).to_string(),
        ))
    }

    fn statuses(etag: &str, states: &[&str]) -> GhOutput {
        let list: Vec<Value> = states.iter().map(|s| json!({"state": s, "context": "ci"})).collect();
        GhOutput::success(include(
            200,
            &[("ETag", etag), ("X-RateLimit-Remaining", "4000")],
            &json!({"state": "pending", "total_count": list.len(), "statuses": list}).to_string(),
        ))
    }

    fn unchanged() -> GhOutput {
        GhOutput { ok: false, stdout: include(304, &[], ""), stderr: "gh: HTTP 304".into() }
    }

    fn target(branch: &str, cached: ChecksStatus) -> Target {
        Target { repo_id: "r".into(), repo_path: "/repo".into(), branch: branch.into(), cached }
    }

    #[test]
    fn suites_without_runs_are_ignored() {
        let body = json!({"check_suites": [
            suite("queued", None, 0),
            suite("completed", Some("success"), 299),
        ]})
        .to_string();
        assert_eq!(suites_bucket(&body), Some(ChecksStatus::Passing));
        let body = json!({"check_suites": [suite("in_progress", None, 22), suite("completed", Some("failure"), 3)]}).to_string();
        assert_eq!(suites_bucket(&body), Some(ChecksStatus::Failing));
        let body = json!({"check_suites": [suite("queued", None, 0)]}).to_string();
        assert_eq!(suites_bucket(&body), Some(ChecksStatus::None));
    }

    #[test]
    fn commit_statuses_map_like_status_contexts() {
        assert_eq!(statuses_bucket(&json!({"statuses": []}).to_string()), Some((ChecksStatus::None, false)));
        let b = json!({"statuses": [{"state": "pending"}, {"state": "success"}]}).to_string();
        assert_eq!(statuses_bucket(&b), Some((ChecksStatus::Pending, true)));
        let b = json!({"statuses": [{"state": "error"}]}).to_string();
        assert_eq!(statuses_bucket(&b), Some((ChecksStatus::Failing, true)));
    }

    #[test]
    fn nudges_only_when_the_bucket_moves() {
        let running = vec![suite("in_progress", None, 5)];
        let gh = FakeGh::new(vec![
            // First look: pending, as GraphQL said → no nudge. No statuses.
            suites("e1", running.clone()),
            statuses("s1", &[]),
            // A run finished but the suite still runs: 200, same bucket.
            suites("e2", running),
            // Nothing changed.
            unchanged(),
            // Done.
            suites("e3", vec![suite("completed", Some("success"), 5)]),
        ]);
        let mut w = CiWatch::default();
        let t = [target("feat/x", ChecksStatus::Pending)];
        let t0 = Instant::now();
        assert!(w.poll(&gh, &t, t0).is_empty());
        assert!(w.poll(&gh, &t, t0 + CI_WATCH_INTERVAL).is_empty());
        assert!(w.poll(&gh, &t, t0 + CI_WATCH_INTERVAL * 2).is_empty());
        assert_eq!(w.poll(&gh, &t, t0 + CI_WATCH_INTERVAL * 3), [("r".to_string(), "feat/x".to_string())]);
        let calls = gh.calls.lock().unwrap();
        assert_eq!(calls.len(), 5, "statuses are only asked again when the branch had some");
        assert!(calls[0].last().unwrap().contains("commits/feat%2Fx/check-suites"));
        assert!(calls[2].contains(&"If-None-Match: e1".to_string()));
        assert!(calls[4].contains(&"If-None-Match: e2".to_string()));
        assert_eq!(w.spent(), 4, "the 304 was free");
    }

    #[test]
    fn a_first_look_that_disagrees_with_graphql_nudges_once() {
        let gh = FakeGh::new(vec![
            suites("e1", vec![suite("completed", Some("success"), 1)]),
            statuses("s1", &[]),
            suites("e2", vec![suite("completed", Some("success"), 1)]),
        ]);
        let mut w = CiWatch::default();
        // GraphQL still says pending (the refetch hasn't landed, or disagrees).
        let t = [target("b", ChecksStatus::Pending)];
        let t0 = Instant::now();
        assert_eq!(w.poll(&gh, &t, t0).len(), 1);
        assert!(w.poll(&gh, &t, t0 + CI_WATCH_INTERVAL).is_empty(), "a lasting disagreement doesn't refetch again");
    }

    #[test]
    fn statuses_are_polled_when_the_branch_has_them() {
        let gh = FakeGh::new(vec![
            suites("e1", vec![]),
            statuses("s1", &["pending"]),
            unchanged(),
            statuses("s2", &["failure"]),
        ]);
        let mut w = CiWatch::default();
        let t = [target("b", ChecksStatus::Pending)];
        let t0 = Instant::now();
        assert!(w.poll(&gh, &t, t0).is_empty());
        assert_eq!(w.poll(&gh, &t, t0 + CI_WATCH_INTERVAL).len(), 1);
        assert!(gh.calls.lock().unwrap()[3].contains(&"If-None-Match: s1".to_string()));
    }

    #[test]
    fn low_rate_limit_pauses() {
        let low = GhOutput::success(include(
            200,
            &[("ETag", "e"), ("X-RateLimit-Remaining", "10")],
            &json!({"check_suites": []}).to_string(),
        ));
        let gh = FakeGh::new(vec![low]);
        let mut w = CiWatch::default();
        let t = [target("a", ChecksStatus::Pending), target("b", ChecksStatus::Pending)];
        let t0 = Instant::now();
        assert!(w.poll(&gh, &t, t0).is_empty());
        assert_eq!(gh.call_count(), 2, "both suites asked at once, then nothing more");
        assert!(!w.due(t0 + SLOW_INTERVAL));
        assert!(w.poll(&gh, &t, t0 + SLOW_INTERVAL).is_empty());
        assert_eq!(gh.call_count(), 2, "nothing while paused");
    }

    #[test]
    fn network_failures_stop_the_poll() {
        let gh = FakeGh::new(vec![GhOutput::failure("error connecting to api.github.com")]);
        let mut w = CiWatch::default();
        let t = [target("a", ChecksStatus::Pending), target("b", ChecksStatus::Pending)];
        let t0 = Instant::now();
        assert!(w.poll(&gh, &t, t0).is_empty());
        assert_eq!(gh.call_count(), 2, "no statuses after a failure");
        assert!(!w.due(t0 + CI_WATCH_INTERVAL));
        assert!(w.due(t0 + SLOW_INTERVAL));
    }

    /// The #169 regression: an hour of constant CI churn on the maximum number
    /// of branches stays inside the budget, and a GraphQL refetch is asked
    /// for only when a bucket moves.
    #[test]
    fn an_hour_of_churn_stays_inside_the_budget() {
        struct Churn {
            calls: std::sync::Mutex<usize>,
        }
        impl GhRunner for Churn {
            fn run(&self, _cwd: &std::path::Path, args: &[String], _t: Duration) -> GhOutput {
                let mut n = self.calls.lock().unwrap();
                *n += 1;
                let path = args.last().unwrap();
                if path.ends_with("/status") {
                    return GhOutput::success(include(200, &[("ETag", "s")], &json!({"statuses": []}).to_string()));
                }
                // Every answer is a 200 with a new ETag; the bucket flips
                // every 40 answers (a run of CI ending, the next push starting).
                let state = if (*n / 40).is_multiple_of(2) { suite("in_progress", None, 9) } else { suite("completed", Some("success"), 9) };
                GhOutput::success(include(200, &[("ETag", &format!("e{n}"))], &json!({"check_suites": [state]}).to_string()))
            }
        }
        let gh = Churn { calls: std::sync::Mutex::new(0) };
        let targets: Vec<Target> = (0..12).map(|i| target(&format!("b{i}"), ChecksStatus::Pending)).collect();
        let mut w = CiWatch::default();
        let t0 = Instant::now();
        let mut now = t0;
        let mut nudges = 0;
        while now < t0 + HOUR {
            if w.due(now) {
                nudges += w.poll(&gh, &targets, now).len();
            }
            now += Duration::from_secs(1);
        }
        let calls = *gh.calls.lock().unwrap();
        assert!(calls <= HOURLY_CAP, "{calls} REST calls in an hour");
        // Each flip moves every watched branch once.
        assert!(nudges <= (calls / 40 + 1) * CI_WATCH_MAX, "{nudges} refetches for {calls} answers");
        assert!(nudges * 4 < calls, "most answers refetch nothing");
    }
}
