//! GitHub PR + Linear ticket integrations with an offline-resilient cache.
//!
//! Port of `src/lib/{github,linear,integration-cache}.ts`, with two upgrades:
//! one batched GraphQL request per repo/chunk instead of one process/request
//! per branch, and a per-source circuit breaker ([`breaker`]) so an offline
//! machine backs off and logs one WARN instead of thousands.
//!
//! Everything here is **blocking**; the TUI calls [`Integrations`] from a
//! worker thread once per `ghPollingInterval` / `linearPollingInterval`:
//!
//! ```no_run
//! # use am_core::integrations::{Integrations, RepoBranches};
//! let conn = am_core::db::open().unwrap();
//! let mut integrations = Integrations::new();
//! integrations.hydrate(&conn); // once, before the first render
//! let repos = vec![RepoBranches::new("repo-id", "/path/to/repo", vec!["main".into(), "feat/x".into()])];
//! integrations.refresh_github(&conn, &repos, false);           // timer tick
//! integrations.refresh_linear(&conn, "lin_api_…", &repos, true); // user pressed `r`
//! let (pr, ticket) = integrations.fields_for("repo-id", "feat/x");
//! let health = integrations.health(); // drives the "⚠ offline — cached" hint
//! # let _ = (pr, ticket, health);
//! ```

pub mod breaker;
pub mod cache;
pub mod github;
pub mod gitpr;
pub mod linear;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use rusqlite::Connection;

use crate::model::{ChecksStatus, LinearInfo, PrInfo};
use crate::{debug, info, warn};
use breaker::Breaker;
use cache::{IntegrationCache, cache_key};
use github::{Detector, GhRunner, SystemGh};
use gitpr::{GitPr, GitRunner, RepoGit, SystemGit};
use linear::{HttpTransport, UreqTransport};

/// How often each repo's PR refs are listed over the git protocol (no API
/// quota); see [`RepoGit::scan_every`] for very large remotes.
pub const GIT_SCAN_EVERY: Duration = Duration::from_secs(30);
/// How often local remote-tracking refs are checked for a push (local, ms).
pub const LOCAL_CHECK_EVERY: Duration = Duration::from_secs(5);
/// Full GraphQL sweep when the REST detector is watching a repo: a safety net
/// only, since the detector reports every change for free.
pub const SAFETY_SWEEP: Duration = Duration::from_secs(15 * 60);
/// Below this many GraphQL points left, sweeps slow down until the reset.
pub const RATE_LOW: u64 = 500;

/// "Absent" vs "failed", exactly like the TS `FetchResult`: `Ok(None)` is a
/// real answer ("this branch has no PR/ticket") that clears the cache;
/// `Err` means we couldn't ask, and the cached value must be kept.
#[derive(Debug, Clone, PartialEq)]
pub enum FetchResult<T> {
    Ok(Option<T>),
    Err(String),
}

impl<T> FetchResult<T> {
    pub fn is_ok(&self) -> bool {
        matches!(self, FetchResult::Ok(_))
    }

    /// Lossy view for one-shot callers that treat failure as absence.
    pub fn value(self) -> Option<T> {
        match self {
            FetchResult::Ok(v) => v,
            FetchResult::Err(_) => None,
        }
    }
}

/// The worktree branches of one repo. Callers pass **every** known repo on
/// each refresh: cache entries for keys outside the list are pruned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoBranches {
    pub repo_id: String,
    pub repo_path: PathBuf,
    pub branches: Vec<String>,
}

impl RepoBranches {
    pub fn new(repo_id: impl Into<String>, repo_path: impl Into<PathBuf>, branches: Vec<String>) -> Self {
        RepoBranches { repo_id: repo_id.into(), repo_path: repo_path.into(), branches }
    }
}

/// Which source's last cycle failed; drives the "⚠ offline — cached" hint.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IntegrationHealth {
    pub github_failing: bool,
    pub linear_failing: bool,
    pub last_github_error: Option<String>,
    pub last_linear_error: Option<String>,
}

/// What a refresh call did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshOutcome {
    /// No branches to fetch.
    Idle,
    /// Circuit open; nothing was fetched and the cache is untouched.
    SkippedBackoff,
    /// Fetches ran. `failed` counts branches whose fetch failed (cache kept);
    /// `aborted` is set when a network failure cut the cycle short.
    Ran { fetched: usize, failed: usize, aborted: bool },
}

/// Facade owning the transports, the in-memory cache and both breakers.
/// Not `Sync`; keep it on the worker thread and send snapshots to the UI.
pub struct Integrations {
    gh: Box<dyn GhRunner>,
    git: Box<dyn GitRunner>,
    http: Box<dyn HttpTransport>,
    linear_url: String,
    cache: IntegrationCache,
    github_breaker: Breaker,
    linear_breaker: Breaker,
    /// Per-repo change tracking for [`Integrations::tick_github`].
    watch: HashMap<String, RepoWatch>,
    rate_low: bool,
}

/// Per-repo state of the event-driven GitHub refresh.
#[derive(Debug, Default)]
struct RepoWatch {
    git: RepoGit,
    detector: Detector,
    /// Branches to fetch on the next tick.
    dirty: HashSet<String>,
    last_sweep: Option<Instant>,
    scan_now: bool,
}

/// One [`Integrations::tick_github`] call (the TUI makes one a second).
#[derive(Debug, Clone, Copy)]
pub struct GithubTick<'a> {
    /// Every repo with its worktree branches (as for [`Integrations::refresh_github`]).
    pub repos: &'a [RepoBranches],
    /// The user pressed `r`: fetch every branch now, bypassing the backoff.
    pub force: bool,
    /// New worktrees, or GitHub was just enabled: sweep (gated by git) now.
    pub sweep: bool,
    /// Plain polling interval (`ghPollingIntervalMs`), used where the
    /// detector isn't available.
    pub interval: Duration,
    /// `(repo_id, branch)` an agent just pushed or ran `gh pr …` in.
    pub hints: &'a [(String, String)],
}

/// What a tick did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickOutcome {
    /// Set when a GraphQL fetch ran.
    pub fetch: Option<RefreshOutcome>,
    /// The PR fields may have changed (fetch or git facts): resend them.
    pub changed: bool,
}

impl Default for Integrations {
    fn default() -> Self {
        Self::new()
    }
}

impl Integrations {
    /// Real `gh` on `$PATH` and ureq against `$AM_LINEAR_API_URL` (or Linear).
    pub fn new() -> Self {
        Self::with_transports(Box::new(SystemGh), Box::new(UreqTransport::default()), linear::linear_api_url())
    }

    /// Inject transports (tests, E2E).
    pub fn with_transports(gh: Box<dyn GhRunner>, http: Box<dyn HttpTransport>, linear_url: String) -> Self {
        Self::with_git(gh, Box::new(SystemGit), http, linear_url)
    }

    /// Inject the git runner too (tests).
    pub fn with_git(gh: Box<dyn GhRunner>, git: Box<dyn GitRunner>, http: Box<dyn HttpTransport>, linear_url: String) -> Self {
        Integrations {
            gh,
            git,
            http,
            linear_url,
            cache: IntegrationCache::new(),
            github_breaker: Breaker::new("github"),
            linear_breaker: Breaker::new("linear"),
            watch: HashMap::new(),
            rate_low: false,
        }
    }

    /// Load the persisted cache (idempotent). Call before the first render so
    /// a cold start with no network still shows PRs/tickets. The refresh
    /// methods also call it.
    pub fn hydrate(&mut self, conn: &Connection) {
        self.cache.hydrate(conn);
    }

    /// Refresh PR info for every branch of every repo: one `gh api graphql`
    /// per repo (per 40 branches). Branches whose fetch failed keep their
    /// cached PR. `force` (user refresh) bypasses the backoff.
    pub fn refresh_github(&mut self, conn: &Connection, repos: &[RepoBranches], force: bool) -> RefreshOutcome {
        self.cache.hydrate(conn);
        let all_keys = all_keys(repos);
        self.cache.prune_prs(conn, &all_keys);
        if all_keys.is_empty() {
            return RefreshOutcome::Idle;
        }
        let now = Instant::now();
        if !self.github_breaker.allow(now, force) {
            return RefreshOutcome::SkippedBackoff;
        }
        self.fetch_github(conn, repos, now).0
    }

    /// Fetch `targets` with the batched GraphQL query, commit the successes
    /// and fold the cycle into the breaker. Returns the outcome and the keys
    /// fetched successfully. The caller checked the breaker.
    fn fetch_github(&mut self, conn: &Connection, targets: &[RepoBranches], now: Instant) -> (RefreshOutcome, HashSet<String>) {
        let (mut fetched, mut failed) = (0, 0);
        let mut first_error: Option<String> = None;
        let mut network_error: Option<String> = None;
        let mut rate_remaining: Option<u64> = None;
        // Fetch everything first, then write in one short transaction: holding a
        // write lock across seconds of `gh` calls would stall the hook writers.
        let mut successes: Vec<(String, FetchResult<PrInfo>)> = Vec::new();
        for repo in targets.iter().filter(|r| !r.branches.is_empty()) {
            if network_error.is_some() {
                // Rest of the cycle aborted; these branches keep their cache.
                failed += distinct(&repo.branches).len();
                continue;
            }
            let batch = github::fetch_prs(self.gh.as_ref(), &repo.repo_path, &repo.branches);
            debug!(
                "github",
                "PR refresh {}: {} branch(es) in {} gh call(s)",
                repo.repo_path.display(),
                batch.results.len(),
                batch.gh_calls
            );
            for (branch, result) in batch.results {
                if let FetchResult::Err(e) = &result {
                    failed += 1;
                    first_error.get_or_insert_with(|| e.clone());
                } else {
                    fetched += 1;
                    successes.push((cache_key(&repo.repo_id, &branch), result));
                }
            }
            network_error = batch.network_error;
            rate_remaining = batch.rate_remaining.or(rate_remaining);
        }
        let tx = conn.unchecked_transaction().ok();
        for (key, result) in &successes {
            self.cache.commit_pr(conn, key, result);
        }
        if let Some(tx) = tx {
            let _ = tx.commit();
        }
        finish(&mut self.github_breaker, now, network_error.as_deref(), first_error.as_deref());
        if let Some(left) = rate_remaining {
            debug!("github", "GraphQL points left this hour: {left}");
            if left < RATE_LOW && !self.rate_low {
                warn!("github", "Only {left} GraphQL points left this hour; slowing PR sweeps until the reset");
            } else if left >= RATE_LOW && self.rate_low {
                info!("github", "GraphQL rate limit recovered ({left} points left)");
            }
            self.rate_low = left < RATE_LOW;
        }
        let ok_keys = successes.into_iter().map(|(k, _)| k).collect();
        (RefreshOutcome::Ran { fetched, failed, aborted: network_error.is_some() }, ok_keys)
    }

    /// Event-driven GitHub refresh, called about once a second. Asks the API
    /// only about branches that can have changed:
    ///
    /// 1. **git** ([`gitpr`]): every [`GIT_SCAN_EVERY`] (sooner after a local
    ///    push or a hook hint) one `git ls-remote` per repo finds PR numbers,
    ///    merges and conflicts with no API quota. A branch whose PR appeared,
    ///    moved or merged becomes dirty.
    /// 2. **REST detector** ([`github::Detector`]): conditional requests whose
    ///    304s are free report PRs and CI runs that changed.
    /// 3. **GraphQL**: the existing batched query, for the dirty branches only;
    ///    plus a sweep every [`SAFETY_SWEEP`] (every `interval` where the
    ///    detector isn't available) that skips branches git shows have no PR
    ///    or a merged one. `force` fetches everything.
    pub fn tick_github(&mut self, conn: &Connection, tick: GithubTick) -> TickOutcome {
        self.cache.hydrate(conn);
        let now = Instant::now();
        let mut out = TickOutcome { fetch: None, changed: false };
        self.watch.retain(|id, _| tick.repos.iter().any(|r| r.repo_id == *id && !r.branches.is_empty()));
        if tick.sweep || tick.force {
            self.cache.prune_prs(conn, &all_keys(tick.repos));
        }
        let mut backoff_ok: Option<bool> = None;
        let mut targets: Vec<RepoBranches> = Vec::new();
        let mut detector_offline = false;

        for repo in tick.repos.iter().filter(|r| !r.branches.is_empty()) {
            let branches = distinct(&repo.branches);
            let w = self.watch.entry(repo.repo_id.clone()).or_default();
            for (_, b) in tick.hints.iter().filter(|(id, b)| *id == repo.repo_id && branches.contains(b)) {
                debug!("github", "hook hint for {b} in {}", repo.repo_path.display());
                w.dirty.insert(b.clone());
                w.scan_now = true;
            }
            if tick.sweep || tick.force {
                w.scan_now = true;
            }

            // 1. git: a push from here, then the remote's PR refs.
            if w.git.last_local.is_none_or(|t| now.duration_since(t) >= LOCAL_CHECK_EVERY) {
                let pushed = w.git.check_local(self.git.as_ref(), &repo.repo_path, &branches, now);
                if !pushed.is_empty() {
                    debug!("github", "push seen for {pushed:?} in {}", repo.repo_path.display());
                    w.dirty.extend(pushed);
                    w.scan_now = true;
                }
            }
            if w.scan_now || w.git.last_scan.is_none_or(|t| now.duration_since(t) >= w.git.scan_every(GIT_SCAN_EVERY)) {
                w.scan_now = false;
                let bases: HashMap<String, String> = branches
                    .iter()
                    .filter_map(|b| {
                        let base = self.cache.pr(&repo.repo_id, b)?.base_ref_name.clone()?;
                        Some((b.clone(), base))
                    })
                    .collect();
                match w.git.scan(self.git.as_ref(), &repo.repo_path, &branches, &bases, now) {
                    Ok(r) => {
                        out.changed |= r.changed;
                        w.dirty.extend(r.dirty);
                    }
                    Err((e, network)) => debug!("gitpr", "ls-remote failed in {} (network={network}): {e}", repo.repo_path.display()),
                }
            }

            // 2. REST detector (gh): free 304s unless something changed.
            if !detector_offline && w.detector.is_due(now) && *backoff_ok.get_or_insert_with(|| self.github_breaker.allow(now, tick.force)) {
                let tracked: HashSet<String> = branches.iter().cloned().collect();
                let d = w.detector.poll(self.gh.as_ref(), &repo.repo_path, &tracked, now);
                if !d.dirty.is_empty() {
                    debug!("github", "detector: {:?} changed in {} ({} call(s))", d.dirty, repo.repo_path.display(), d.calls);
                }
                w.dirty.extend(d.dirty);
                if let Some(e) = d.network_error {
                    debug!("github", "detector offline: {e}");
                    detector_offline = true;
                }
            }

            // 3. What to fetch.
            let mut every = if w.detector.is_primed() { SAFETY_SWEEP.max(tick.interval) } else { tick.interval };
            if self.rate_low {
                every *= 4;
            }
            let sweep_due = tick.force || tick.sweep || w.last_sweep.is_none_or(|t| now.duration_since(t) >= every);
            let want: Vec<String> = if tick.force {
                branches.clone()
            } else if sweep_due {
                branches.iter().filter(|b| !skip_in_sweep(&w.git, self.cache.pr(&repo.repo_id, b), b)).cloned().collect()
            } else {
                branches.iter().filter(|b| w.dirty.contains(*b)).cloned().collect()
            };
            if sweep_due {
                w.last_sweep = Some(now);
                debug!(
                    "github",
                    "sweep {}: {} of {} branch(es) need the API",
                    repo.repo_path.display(),
                    want.len(),
                    branches.len()
                );
            }
            if !want.is_empty() {
                targets.push(RepoBranches::new(repo.repo_id.clone(), repo.repo_path.clone(), want));
            }
        }

        if !targets.is_empty() && *backoff_ok.get_or_insert_with(|| self.github_breaker.allow(now, tick.force)) {
            let (outcome, ok_keys) = self.fetch_github(conn, &targets, now);
            for t in &targets {
                if let Some(w) = self.watch.get_mut(&t.repo_id) {
                    w.dirty.retain(|b| !ok_keys.contains(&cache_key(&t.repo_id, b)));
                }
            }
            out.fetch = Some(outcome);
            out.changed = true;
        }
        out
    }

    /// Refresh Linear tickets. `issueVcsBranchSearch` is workspace-wide, so each
    /// distinct branch is fetched once (batched) and fanned out to every
    /// repo-scoped key. Failures keep the cached ticket.
    pub fn refresh_linear(
        &mut self,
        conn: &Connection,
        api_key: &str,
        repos: &[RepoBranches],
        force: bool,
    ) -> RefreshOutcome {
        self.cache.hydrate(conn);
        let all_keys = all_keys(repos);
        self.cache.prune_linear(conn, &all_keys);
        if all_keys.is_empty() {
            return RefreshOutcome::Idle;
        }
        let now = Instant::now();
        if !self.linear_breaker.allow(now, force) {
            return RefreshOutcome::SkippedBackoff;
        }

        let branches = distinct(repos.iter().flat_map(|r| r.branches.iter()));
        let batch = linear::fetch_linear_batch(self.http.as_ref(), &self.linear_url, api_key, &branches);
        debug!("linear", "Linear refresh: {} branch(es) in {} request(s)", branches.len(), batch.requests);

        let (mut fetched, mut failed) = (0, 0);
        let mut first_error: Option<String> = None;
        let tx = conn.unchecked_transaction().ok();
        for repo in repos {
            for branch in distinct(&repo.branches) {
                let Some((_, result)) = batch.results.iter().find(|(b, _)| *b == branch) else { continue };
                if let FetchResult::Err(e) = result {
                    failed += 1;
                    first_error.get_or_insert_with(|| e.clone());
                } else {
                    fetched += 1;
                    self.cache.commit_linear(conn, &cache_key(&repo.repo_id, &branch), result);
                }
            }
        }
        if let Some(tx) = tx {
            let _ = tx.commit();
        }
        finish(&mut self.linear_breaker, now, batch.network_error.as_deref(), first_error.as_deref());
        RefreshOutcome::Ran { fetched, failed, aborted: batch.network_error.is_some() }
    }

    /// `pr_info` / `linear_info` for one worktree: the gh PR, else the PR
    /// attached to its Linear ticket (port of `integrationFieldsFor`).
    /// Git-derived facts ([`gitpr`]) are fresher than the cache: they show a
    /// new PR's number, a merge or a conflict before the API is asked.
    pub fn fields_for(&self, repo_id: &str, branch: &str) -> (Option<PrInfo>, Option<LinearInfo>) {
        let (pr, linear) = self.cache.fields_for(repo_id, branch);
        let pr = match self.watch.get(repo_id).and_then(|w| w.git.fact(branch).map(|f| (w, f))) {
            Some((w, fact)) => overlay(pr, fact, || w.git.pr_url(fact.number)),
            None => pr,
        };
        (pr, linear)
    }

    /// The gh-sourced PR only (no Linear fallback).
    pub fn cached_pr(&self, repo_id: &str, branch: &str) -> Option<&PrInfo> {
        self.cache.pr(repo_id, branch)
    }

    /// Cached Linear ticket (e.g. for auto-nicknaming).
    pub fn cached_linear(&self, repo_id: &str, branch: &str) -> Option<&LinearInfo> {
        self.cache.linear(repo_id, branch)
    }

    /// Forget all Linear tickets (memory + disk) and Linear's failure state —
    /// for when Linear is disabled in settings.
    pub fn clear_linear(&mut self, conn: &Connection) {
        self.cache.clear_linear(conn);
        self.linear_breaker = Breaker::new("linear");
    }

    pub fn health(&self) -> IntegrationHealth {
        IntegrationHealth {
            github_failing: self.github_breaker.is_failing(),
            linear_failing: self.linear_breaker.is_failing(),
            last_github_error: self.github_breaker.last_error().map(str::to_string),
            last_linear_error: self.linear_breaker.last_error().map(str::to_string),
        }
    }

    /// One-shot lookup for CLI commands (`am pr show`): no cache, no breaker.
    pub fn fetch_pr_now(&self, repo_path: &std::path::Path, branch: &str) -> FetchResult<PrInfo> {
        let mut batch = github::fetch_prs(self.gh.as_ref(), repo_path, &[branch.to_string()]);
        batch.results.pop().map(|(_, r)| r).unwrap_or(FetchResult::Ok(None))
    }

    /// One-shot lookup for CLI commands (`am linear show`): no cache, no breaker.
    pub fn fetch_linear_now(&self, api_key: &str, branch: &str) -> FetchResult<LinearInfo> {
        linear::fetch_linear_result(self.http.as_ref(), &self.linear_url, api_key, branch)
    }

    /// Port of `verifyLinearApiKey`: the viewer's name, or Linear's message.
    pub fn verify_linear_key(&self, api_key: &str) -> Result<String, String> {
        linear::verify_api_key(self.http.as_ref(), &self.linear_url, api_key)
    }
}

/// Sweeps skip what git settles: no PR ref and nothing cached (no PR), or a
/// merge the API already confirmed (terminal).
fn skip_in_sweep(git: &RepoGit, cached: Option<&PrInfo>, branch: &str) -> bool {
    if !git.usable() {
        return false;
    }
    match (git.fact(branch), cached) {
        (None, None) => true,
        (Some(f), Some(p)) => f.merged && p.number == f.number && p.state == "MERGED",
        _ => false,
    }
}

/// Lay a git fact over the cached PR (or synthesise one until the API answers).
fn overlay(pr: Option<PrInfo>, fact: GitPr, url: impl FnOnce() -> Option<String>) -> Option<PrInfo> {
    match pr {
        Some(mut p) if p.number == fact.number => {
            if p.state == "OPEN" {
                if fact.merged {
                    p.state = "MERGED".into();
                } else if let Some(c) = fact.conflicts {
                    if c {
                        p.mergeable = Some("CONFLICTING".into());
                    } else if p.mergeable.as_deref() == Some("CONFLICTING") {
                        p.mergeable = Some("MERGEABLE".into());
                    }
                }
            }
            Some(p)
        }
        // The API knows a newer PR than the refs we listed.
        Some(p) if p.number > fact.number => Some(p),
        _ => Some(PrInfo {
            number: fact.number,
            title: String::new(),
            url: url().unwrap_or_default(),
            state: if fact.merged { "MERGED" } else { "OPEN" }.into(),
            is_draft: false,
            review_decision: String::new(),
            has_feedback: false,
            checks_status: ChecksStatus::None,
            active_check_url: None,
            active_check_name: None,
            checks_waiting: false,
            base_ref_name: None,
            mergeable: fact.conflicts.map(|c| if c { "CONFLICTING" } else { "MERGEABLE" }.into()),
        }),
    }
}

fn all_keys(repos: &[RepoBranches]) -> Vec<String> {
    repos.iter().flat_map(|r| r.branches.iter().map(|b| cache_key(&r.repo_id, b))).collect()
}

fn distinct<'a>(branches: impl IntoIterator<Item = &'a String>) -> Vec<String> {
    let mut seen = HashSet::new();
    branches.into_iter().filter(|b| seen.insert(b.as_str())).cloned().collect()
}

/// Fold one cycle into the breaker: a single transition per cycle, so a
/// partially failing source logs once rather than flapping per branch.
fn finish(b: &mut Breaker, now: Instant, network_error: Option<&str>, first_error: Option<&str>) {
    match (network_error, first_error) {
        (Some(e), _) => b.on_failure(now, e, true),
        (None, Some(e)) => b.on_failure(now, e, false),
        (None, None) => b.on_success(),
    }
}

#[cfg(test)]
mod tests {
    use super::github::GhOutput;
    use super::github::tests::{FakeGh, gql_node, gql_response};
    use super::linear::tests::{FakeHttp, aliased, issue, offline, ok, status};
    use super::*;
    use crate::model::ChecksStatus;
    use serde_json::{Value, json};

    struct Env {
        _dir: tempfile::TempDir,
        conn: Connection,
        gh: FakeGh,
        http: FakeHttp,
        ints: Integrations,
    }

    fn env() -> Env {
        let dir = tempfile::tempdir().unwrap();
        let conn = crate::db::open_at(&dir.path().join("t.db")).unwrap();
        let gh = FakeGh::default();
        let http = FakeHttp::default();
        let ints = Integrations::with_transports(Box::new(gh.clone()), Box::new(http.clone()), "http://linear".into());
        Env { _dir: dir, conn, gh, http, ints }
    }

    impl Env {
        /// A fresh facade over the same DB — the TUI relaunching.
        fn restart(&mut self) {
            self.ints = Integrations::with_transports(
                Box::new(self.gh.clone()),
                Box::new(self.http.clone()),
                "http://linear".into(),
            );
        }
        fn rows(&self, kind: cache::CacheKind) -> Vec<String> {
            let mut keys: Vec<String> =
                cache::load_rows(&self.conn, kind).unwrap().into_iter().map(|r| r.cache_key).collect();
            keys.sort();
            keys
        }
    }

    fn repo() -> Vec<RepoBranches> {
        vec![RepoBranches::new("r1", "/repo", vec!["feature/x".into()])]
    }

    fn linear_ok(id: &str) -> Result<linear::HttpResponse, linear::TransportError> {
        ok(aliased(vec![issue(id)]))
    }

    fn gh_pr(number: u64, state: &str) -> GhOutput {
        GhOutput::success(gql_response(vec![vec![gql_node(number, "feature/x", state, json!([]))]]))
    }

    fn gh_none() -> GhOutput {
        GhOutput::success(gql_response(vec![vec![]]))
    }

    fn gh_offline() -> GhOutput {
        GhOutput::failure("error connecting to api.github.com\ncheck your internet connection")
    }

    // --- Linear ---

    #[test]
    fn linear_caches_in_memory_and_on_disk() {
        let mut e = env();
        e.http.push(linear_ok("ENG-1"));
        e.ints.refresh_linear(&e.conn, "key", &repo(), false);
        assert_eq!(e.ints.cached_linear("r1", "feature/x").unwrap().identifier, "ENG-1");
        let rows = cache::load_rows(&e.conn, cache::CacheKind::Linear).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].cache_key, "r1:feature/x");
        // payload is the TS-compatible camelCase JSON
        let v: Value = serde_json::from_str(rows[0].payload.as_deref().unwrap()).unwrap();
        assert_eq!(v["identifier"], "ENG-1");
        assert_eq!(v["state"]["type"], "started");
        assert_eq!(v["priorityLabel"], "High");
        assert_eq!(v["prAttachment"]["metadata"]["number"], 42);
    }

    #[test]
    fn linear_keeps_cached_ticket_when_fetch_fails() {
        let mut e = env();
        e.http.push(linear_ok("ENG-1"));
        e.ints.refresh_linear(&e.conn, "key", &repo(), false);
        e.http.push(status(500, json!({})));
        e.ints.refresh_linear(&e.conn, "key", &repo(), false);
        assert_eq!(e.ints.cached_linear("r1", "feature/x").unwrap().identifier, "ENG-1");
        assert!(e.ints.health().linear_failing);
    }

    #[test]
    fn linear_replaces_on_later_success_and_clears_on_none() {
        let mut e = env();
        e.http.push(linear_ok("ENG-1"));
        e.ints.refresh_linear(&e.conn, "key", &repo(), false);
        e.http.push(linear_ok("ENG-2"));
        e.ints.refresh_linear(&e.conn, "key", &repo(), false);
        assert_eq!(e.ints.cached_linear("r1", "feature/x").unwrap().identifier, "ENG-2");
        e.http.push(ok(aliased(vec![Value::Null])));
        e.ints.refresh_linear(&e.conn, "key", &repo(), false);
        assert!(e.ints.cached_linear("r1", "feature/x").is_none());
        let rows = cache::load_rows(&e.conn, cache::CacheKind::Linear).unwrap();
        assert_eq!(rows[0].payload, None, "absence is persisted as NULL");
    }

    #[test]
    fn linear_fetches_once_per_distinct_branch_across_repos() {
        let mut e = env();
        let repos = vec![
            RepoBranches::new("r1", "/a", vec!["feature/x".into(), "feature/y".into()]),
            RepoBranches::new("r2", "/b", vec!["feature/x".into()]),
        ];
        e.http.push(ok(aliased(vec![issue("ENG-1"), issue("ENG-2")])));
        e.ints.refresh_linear(&e.conn, "key", &repos, false);
        assert_eq!(e.http.request_count(), 1);
        let body: Value = serde_json::from_str(&e.http.bodies.lock().unwrap()[0]).unwrap();
        assert_eq!(body["variables"], json!({"b0": "feature/x", "b1": "feature/y"}));
        assert_eq!(e.ints.cached_linear("r1", "feature/x").unwrap().identifier, "ENG-1");
        assert_eq!(e.ints.cached_linear("r2", "feature/x").unwrap().identifier, "ENG-1");
        assert_eq!(e.ints.cached_linear("r1", "feature/y").unwrap().identifier, "ENG-2");
        assert!(e.ints.cached_linear("r2", "feature/y").is_none());
    }

    // --- GitHub ---

    #[test]
    fn github_keeps_cached_pr_when_fetch_fails() {
        let mut e = env();
        e.gh.push(gh_pr(42, "OPEN"));
        e.ints.refresh_github(&e.conn, &repo(), false);
        e.gh.push(GhOutput::failure("HTTP 401: Bad credentials"));
        e.gh.push(GhOutput::failure("HTTP 401: Bad credentials")); // pr view fallback
        let out = e.ints.refresh_github(&e.conn, &repo(), false);
        assert_eq!(out, RefreshOutcome::Ran { fetched: 0, failed: 1, aborted: false });
        assert_eq!(e.ints.cached_pr("r1", "feature/x").unwrap().number, 42);
        let h = e.ints.health();
        assert!(h.github_failing && !h.linear_failing);
        assert_eq!(h.last_github_error.as_deref(), Some("HTTP 401: Bad credentials"));
    }

    #[test]
    fn github_clears_cached_pr_when_branch_has_none() {
        let mut e = env();
        e.gh.push(gh_pr(42, "OPEN"));
        e.ints.refresh_github(&e.conn, &repo(), false);
        e.gh.push(gh_none());
        e.ints.refresh_github(&e.conn, &repo(), false);
        assert!(e.ints.fields_for("r1", "feature/x").0.is_none());
    }

    #[test]
    fn github_one_gh_call_per_repo() {
        let mut e = env();
        let repos = vec![
            RepoBranches::new("r1", "/a", vec!["feature/x".into(), "b".into(), "c".into()]),
            RepoBranches::new("r2", "/b", vec!["feature/x".into()]),
        ];
        e.gh.push(GhOutput::success(gql_response(vec![
            vec![gql_node(1, "feature/x", "OPEN", json!([]))],
            vec![],
            vec![],
        ])));
        e.gh.push(GhOutput::success(gql_response(vec![vec![gql_node(2, "feature/x", "MERGED", json!([]))]])));
        e.ints.refresh_github(&e.conn, &repos, false);
        assert_eq!(e.gh.call_count(), 2);
        assert_eq!(e.ints.cached_pr("r1", "feature/x").unwrap().number, 1);
        assert_eq!(e.ints.cached_pr("r2", "feature/x").unwrap().number, 2);
        assert_eq!(e.ints.cached_pr("r2", "feature/x").unwrap().checks_status, ChecksStatus::None);
    }

    #[test]
    fn github_network_failure_aborts_cycle_and_backs_off() {
        let mut e = env();
        let repos = vec![
            RepoBranches::new("r1", "/a", vec!["feature/x".into()]),
            RepoBranches::new("r2", "/b", vec!["feature/x".into()]),
        ];
        e.gh.push(gh_offline());
        let out = e.ints.refresh_github(&e.conn, &repos, false);
        assert_eq!(out, RefreshOutcome::Ran { fetched: 0, failed: 2, aborted: true });
        assert_eq!(e.gh.call_count(), 1, "second repo not attempted");

        // Background poll while backing off: nothing runs.
        assert_eq!(e.ints.refresh_github(&e.conn, &repos, false), RefreshOutcome::SkippedBackoff);
        assert_eq!(e.gh.call_count(), 1);
        assert!(e.ints.health().github_failing);

        // A user refresh bypasses the backoff and recovery clears the hint.
        e.gh.push(gh_pr(1, "OPEN"));
        e.gh.push(gh_pr(2, "OPEN"));
        let out = e.ints.refresh_github(&e.conn, &repos, true);
        assert_eq!(out, RefreshOutcome::Ran { fetched: 2, failed: 0, aborted: false });
        assert_eq!(e.ints.health(), IntegrationHealth::default());
    }

    #[test]
    fn linear_network_failure_backs_off_until_forced() {
        let mut e = env();
        e.http.push(offline());
        e.ints.refresh_linear(&e.conn, "key", &repo(), false);
        assert_eq!(e.ints.refresh_linear(&e.conn, "key", &repo(), false), RefreshOutcome::SkippedBackoff);
        assert_eq!(e.http.request_count(), 1);
        e.http.push(linear_ok("ENG-1"));
        assert!(matches!(e.ints.refresh_linear(&e.conn, "key", &repo(), true), RefreshOutcome::Ran { .. }));
        assert!(!e.ints.health().linear_failing);
    }

    #[test]
    fn falls_back_to_the_pr_attached_to_the_linear_ticket() {
        let mut e = env();
        let mut i = issue("ENG-1");
        i["attachments"]["nodes"][0]["metadata"] = json!({"number": 7, "draft": false, "mergedAt": null, "closedAt": null});
        e.http.push(ok(aliased(vec![i])));
        e.ints.refresh_linear(&e.conn, "key", &repo(), false);
        let (pr, ticket) = e.ints.fields_for("r1", "feature/x");
        assert_eq!(pr.unwrap().number, 7);
        assert_eq!(ticket.unwrap().identifier, "ENG-1");
        assert!(e.ints.cached_pr("r1", "feature/x").is_none());
    }

    // --- health ---

    #[test]
    fn health_flags_failing_source_and_clears_on_recovery() {
        let mut e = env();
        e.http.push(linear_ok("ENG-1"));
        e.ints.refresh_linear(&e.conn, "key", &repo(), false);
        assert!(!e.ints.health().linear_failing);
        e.http.push(ok(json!({"errors": [{"message": "Rate limited"}]})));
        e.ints.refresh_linear(&e.conn, "key", &repo(), false);
        let h = e.ints.health();
        assert!(h.linear_failing && !h.github_failing);
        assert_eq!(h.last_linear_error.as_deref(), Some("Rate limited"));
        e.http.push(linear_ok("ENG-1"));
        e.ints.refresh_linear(&e.conn, "key", &repo(), false);
        assert!(!e.ints.health().linear_failing);
    }

    // --- persistence ---

    #[test]
    fn serves_persisted_data_on_a_cold_start_with_no_network() {
        let mut e = env();
        e.http.push(linear_ok("ENG-1"));
        e.gh.push(gh_pr(42, "OPEN"));
        e.ints.refresh_linear(&e.conn, "key", &repo(), false);
        e.ints.refresh_github(&e.conn, &repo(), false);

        e.restart();
        e.ints.hydrate(&e.conn);
        let (pr, ticket) = e.ints.fields_for("r1", "feature/x");
        assert_eq!(ticket.unwrap().identifier, "ENG-1");
        assert_eq!(pr.unwrap().number, 42);
        assert_eq!(e.gh.call_count(), 1);
        assert_eq!(e.http.request_count(), 1);
    }

    #[test]
    fn reads_payloads_written_by_the_ts_app() {
        let e = env();
        let ts_pr = r#"{"number":127,"title":"t","url":"u","state":"OPEN","isDraft":false,"reviewDecision":"APPROVED","hasFeedback":false,"checksStatus":"failing","activeCheckUrl":"x","activeCheckName":"test","checksWaiting":false}"#;
        cache::set_entry(&e.conn, cache::CacheKind::Pr, "r1:feature/x", Some(ts_pr)).unwrap();
        let mut ints = e.ints;
        ints.hydrate(&e.conn);
        let pr = ints.cached_pr("r1", "feature/x").unwrap();
        assert_eq!(pr.checks_status, ChecksStatus::Failing);
        // …and what we write back is the same JSON shape.
        assert_eq!(serde_json::to_string(pr).unwrap(), ts_pr);
    }

    #[test]
    fn hydrates_at_most_once() {
        let mut e = env();
        e.http.push(linear_ok("ENG-1"));
        e.ints.refresh_linear(&e.conn, "key", &repo(), false);
        e.restart();
        e.ints.hydrate(&e.conn);
        cache::set_entry(&e.conn, cache::CacheKind::Linear, "r1:other", None).unwrap();
        e.conn.execute("DELETE FROM integration_cache WHERE cache_key = 'r1:feature/x'", []).unwrap();
        e.ints.hydrate(&e.conn); // no re-read
        assert_eq!(e.ints.cached_linear("r1", "feature/x").unwrap().identifier, "ENG-1");
    }

    #[test]
    fn survives_a_corrupt_payload() {
        let mut e = env();
        cache::set_entry(&e.conn, cache::CacheKind::Linear, "r1:feature/x", Some("{{not json")).unwrap();
        e.ints.hydrate(&e.conn);
        assert!(e.ints.cached_linear("r1", "feature/x").is_none());
    }

    #[test]
    fn prunes_entries_whose_worktree_is_gone() {
        let mut e = env();
        let two = vec![RepoBranches::new("r1", "/repo", vec!["feature/x".into(), "feature/gone".into()])];
        e.http.push(ok(aliased(vec![issue("ENG-1"), Value::Null])));
        e.ints.refresh_linear(&e.conn, "key", &two, false);
        assert_eq!(e.rows(cache::CacheKind::Linear).len(), 2);
        e.http.push(linear_ok("ENG-1"));
        e.ints.refresh_linear(&e.conn, "key", &repo(), false);
        assert_eq!(e.rows(cache::CacheKind::Linear), vec!["r1:feature/x"]);
    }

    #[test]
    fn empty_worktree_list_does_not_wipe_the_cache() {
        let mut e = env();
        e.http.push(linear_ok("ENG-1"));
        e.ints.refresh_linear(&e.conn, "key", &repo(), false);
        let empty = vec![RepoBranches::new("r1", "/repo", vec![])];
        assert_eq!(e.ints.refresh_linear(&e.conn, "key", &empty, false), RefreshOutcome::Idle);
        assert_eq!(e.ints.refresh_github(&e.conn, &[], false), RefreshOutcome::Idle);
        assert_eq!(e.rows(cache::CacheKind::Linear).len(), 1);
        assert_eq!(e.ints.cached_linear("r1", "feature/x").unwrap().identifier, "ENG-1");
    }

    #[test]
    fn clear_linear_forgets_tickets_without_touching_prs() {
        let mut e = env();
        e.http.push(linear_ok("ENG-1"));
        e.gh.push(gh_pr(42, "OPEN"));
        e.ints.refresh_linear(&e.conn, "key", &repo(), false);
        e.ints.refresh_github(&e.conn, &repo(), false);
        e.ints.clear_linear(&e.conn);
        let (pr, ticket) = e.ints.fields_for("r1", "feature/x");
        assert!(ticket.is_none());
        assert_eq!(pr.unwrap().number, 42);
        assert!(e.rows(cache::CacheKind::Linear).is_empty());
        assert_eq!(e.rows(cache::CacheKind::Pr).len(), 1);
    }

    // --- event-driven refresh (tick_github) ---

    /// Scripted git: answers by subcommand.
    #[derive(Clone, Default)]
    struct FakeGit {
        ls_remote: Arc<Mutex<String>>,
        local: Arc<Mutex<String>>,
        subjects: Arc<Mutex<String>>,
        calls: Arc<Mutex<Vec<Vec<String>>>>,
    }

    impl gitpr::GitRunner for FakeGit {
        fn run(&self, _cwd: &std::path::Path, args: &[String], _t: Duration) -> GhOutput {
            self.calls.lock().unwrap().push(args.to_vec());
            match args[0].as_str() {
                "ls-remote" => GhOutput::success(self.ls_remote.lock().unwrap().clone()),
                "for-each-ref" => GhOutput::success(self.local.lock().unwrap().clone()),
                "log" => GhOutput::success(self.subjects.lock().unwrap().clone()),
                "remote" => GhOutput::success("git@github.com:o/r.git\n"),
                "merge-tree" => GhOutput::success("tree\n"),
                _ => GhOutput::failure("not scripted"),
            }
        }
    }

    use std::sync::{Arc, Mutex};

    fn tick_env() -> (Env, FakeGit) {
        let mut e = env();
        let git = FakeGit::default();
        *git.ls_remote.lock().unwrap() = "ref: refs/heads/main\tHEAD\n\
             aaa\trefs/heads/main\n\
             bbb\trefs/heads/feat/a\n\
             ccc\trefs/heads/feat/b\n\
             bbb\trefs/pull/5/head\n\
             zzz\trefs/pull/1/head\n"
            .into();
        *git.local.lock().unwrap() = "aaa refs/remotes/origin/main\nbbb refs/heads/feat/a\nccc refs/heads/feat/b\n".into();
        *git.subjects.lock().unwrap() = "feat: a (#5)\ninit\n".into();
        e.ints = Integrations::with_git(Box::new(e.gh.clone()), Box::new(git.clone()), Box::new(e.http.clone()), "http://linear".into());
        (e, git)
    }

    fn tick_repos() -> Vec<RepoBranches> {
        vec![RepoBranches::new("r1", "/repo", vec!["main".into(), "feat/a".into(), "feat/b".into()])]
    }

    fn tick<'a>(repos: &'a [RepoBranches], sweep: bool, hints: &'a [(String, String)]) -> GithubTick<'a> {
        GithubTick { repos, force: false, sweep, interval: Duration::from_secs(3600), hints }
    }

    fn graphql_calls(gh: &FakeGh) -> Vec<Vec<String>> {
        gh.calls.lock().unwrap().iter().filter(|a| a[..2] == ["api", "graphql"]).cloned().collect()
    }

    fn asks_for(call: &[String], branch: &str) -> bool {
        call.iter().any(|a| a.split_once('=').is_some_and(|(k, v)| k.starts_with('b') && v == branch))
    }

    #[test]
    fn tick_asks_the_api_only_about_what_git_cannot_settle() {
        let (mut e, _git) = tick_env();
        let repos = tick_repos();
        // Detector unavailable (no status line): plain sweeps, gated by git.
        e.gh.push(GhOutput::success(""));
        e.gh.push(GhOutput::success(gql_response(vec![vec![gql_node(5, "feat/a", "MERGED", json!([]))]])));
        let out = e.ints.tick_github(&e.conn, tick(&repos, true, &[]));
        assert!(out.changed);
        let calls = graphql_calls(&e.gh);
        assert_eq!(calls.len(), 1);
        // Only feat/a: its merge needs the API to confirm once. main and feat/b
        // have no PR ref (and nothing cached), so git already answered.
        assert!(asks_for(&calls[0], "feat/a"));
        assert!(!asks_for(&calls[0], "main") && !asks_for(&calls[0], "feat/b"));
        assert_eq!(e.ints.fields_for("r1", "feat/a").0.unwrap().state, "MERGED");

        // The next sweep has nothing git can't answer: merged + confirmed is terminal.
        let out = e.ints.tick_github(&e.conn, tick(&repos, true, &[]));
        assert_eq!(out.fetch, None);
        assert_eq!(graphql_calls(&e.gh).len(), 1);

        // Between sweeps a quiet tick costs nothing.
        let before = e.gh.call_count();
        let out = e.ints.tick_github(&e.conn, tick(&repos, false, &[]));
        assert_eq!(out, TickOutcome { fetch: None, changed: false });
        assert_eq!(e.gh.call_count(), before);
    }

    #[test]
    fn tick_shows_a_new_pr_from_git_before_the_api_answers() {
        let (mut e, git) = tick_env();
        let repos = tick_repos();
        e.gh.push(GhOutput::success("")); // detector unavailable
        e.gh.push(gh_offline()); // the API is down
        e.ints.tick_github(&e.conn, tick(&repos, true, &[]));
        // PR #5's merge shows from git alone, with a URL built from the remote.
        let pr = e.ints.fields_for("r1", "feat/a").0.unwrap();
        assert_eq!((pr.number, pr.state.as_str()), (5, "MERGED"));
        assert_eq!(pr.url, "https://github.com/o/r/pull/5");

        // feat/b gets PR #8 (not merged, no conflicts).
        let ls = git.ls_remote.lock().unwrap().clone() + "ccc\trefs/pull/8/head\n";
        *git.ls_remote.lock().unwrap() = ls;
        let hint = vec![("r1".to_string(), "feat/b".to_string())];
        e.gh.push(GhOutput::success(gql_response(vec![vec![gql_node(8, "feat/b", "OPEN", json!([]))]])));
        // Backoff is open after the offline fetch; a user refresh goes through.
        let out = e.ints.tick_github(&e.conn, GithubTick { force: true, ..tick(&repos, false, &hint) });
        assert!(out.fetch.is_some());
        let pr = e.ints.fields_for("r1", "feat/b").0.unwrap();
        assert_eq!((pr.number, pr.state.as_str(), pr.mergeable.as_deref()), (8, "OPEN", Some("MERGEABLE")));
    }

    #[test]
    fn tick_fetches_a_hinted_branch_outside_sweeps() {
        let (mut e, _git) = tick_env();
        let repos = tick_repos();
        e.gh.push(GhOutput::success(""));
        e.gh.push(GhOutput::success(gql_response(vec![vec![gql_node(5, "feat/a", "MERGED", json!([]))]])));
        e.ints.tick_github(&e.conn, tick(&repos, true, &[]));
        let hint = vec![("r1".to_string(), "feat/b".to_string())];
        e.gh.push(GhOutput::success(gql_response(vec![vec![]])));
        let out = e.ints.tick_github(&e.conn, tick(&repos, false, &hint));
        assert!(out.fetch.is_some());
        let calls = graphql_calls(&e.gh);
        assert_eq!(calls.len(), 2);
        assert!(asks_for(&calls[1], "feat/b") && !asks_for(&calls[1], "main") && !asks_for(&calls[1], "feat/a"));
    }

    #[test]
    fn overlay_rules() {
        let fact = |merged, conflicts| GitPr { number: 5, merged, conflicts };
        let open = |n: u64| PrInfo {
            number: n,
            title: "t".into(),
            url: "u".into(),
            state: "OPEN".into(),
            is_draft: true,
            review_decision: String::new(),
            has_feedback: false,
            checks_status: ChecksStatus::Passing,
            active_check_url: None,
            active_check_name: None,
            checks_waiting: false,
            base_ref_name: None,
            mergeable: Some("CONFLICTING".into()),
        };
        let p = overlay(Some(open(5)), fact(true, None), || None).unwrap();
        assert_eq!(p.state, "MERGED");
        let p = overlay(Some(open(5)), fact(false, Some(false)), || None).unwrap();
        assert_eq!((p.state.as_str(), p.mergeable.as_deref(), p.is_draft), ("OPEN", Some("MERGEABLE"), true));
        let p = overlay(Some(open(9)), fact(true, None), || None).unwrap();
        assert_eq!((p.number, p.state.as_str()), (9, "OPEN"), "a newer PR from the API wins");
        let mut closed = open(5);
        closed.state = "CLOSED".into();
        assert_eq!(overlay(Some(closed), fact(false, None), || None).unwrap().state, "CLOSED");
        let p = overlay(Some(open(3)), fact(false, Some(true)), || Some("url".into())).unwrap();
        assert_eq!((p.number, p.title.as_str(), p.url.as_str(), p.mergeable.as_deref()), (5, "", "url", Some("CONFLICTING")));
    }

    #[test]
    fn one_shot_lookups_bypass_cache() {
        let e = env();
        e.gh.push(gh_pr(5, "OPEN"));
        let r = e.ints.fetch_pr_now(std::path::Path::new("/repo"), "feature/x");
        assert_eq!(r.value().unwrap().number, 5);
        e.http.push(linear_ok("ENG-9"));
        assert_eq!(e.ints.fetch_linear_now("k", "feature/x").value().unwrap().identifier, "ENG-9");
        assert!(e.ints.cached_pr("r1", "feature/x").is_none());
    }
}
