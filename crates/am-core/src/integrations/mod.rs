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
pub mod linear;
pub mod reviews;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use rusqlite::Connection;

use crate::{debug, info};
use crate::model::{LinearInfo, PrInfo, ReviewFetchError, ReviewPerson, ReviewRequest};
use crate::settings::Settings;
use breaker::Breaker;
use cache::{IntegrationCache, cache_key};
use github::{GhRunner, SystemGh};
use linear::{HttpTransport, UreqTransport};

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
    http: Box<dyn HttpTransport>,
    linear_url: String,
    cache: IntegrationCache,
    github_breaker: Breaker,
    linear_breaker: Breaker,
    /// Last successfully fetched review requests (`None` = never fetched).
    reviews: Option<Vec<ReviewRequest>>,
    /// Why the last review search failed; cleared by a success.
    review_error: Option<ReviewFetchError>,
    reviews_hydrated: bool,
    /// Open PRs requesting your or your teams' review on GitHub, ignoring the
    /// filters (only fetched while the badge shows it; cached).
    review_total: Option<u64>,
    /// Which PRs match each review group's GitHub search terms (cached).
    review_hits: crate::review_groups::QueryHits,
    /// `owner/name` per repo path (only successes are kept: a failure may be
    /// a missing `gh auth login` that the next refresh gets past).
    slugs: HashMap<PathBuf, String>,
    /// Who can author PRs in the tracked repos (autocomplete; cached).
    people: Vec<ReviewPerson>,
    /// When `people` was last fetched by this process.
    people_at: Option<Instant>,
}

/// How long a people lookup is reused before asking GitHub again.
pub const PEOPLE_TTL: Duration = Duration::from_secs(3600);

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
        Integrations {
            gh,
            http,
            linear_url,
            cache: IntegrationCache::new(),
            github_breaker: Breaker::new("github"),
            linear_breaker: Breaker::new("linear"),
            reviews: None,
            review_error: None,
            reviews_hydrated: false,
            review_total: None,
            review_hits: Default::default(),
            slugs: HashMap::new(),
            people: Vec::new(),
            people_at: None,
        }
    }

    /// Load the persisted cache (idempotent). Call before the first render so
    /// a cold start with no network still shows PRs/tickets. The refresh
    /// methods also call it.
    pub fn hydrate(&mut self, conn: &Connection) {
        self.cache.hydrate(conn);
        if !self.reviews_hydrated {
            self.reviews_hydrated = true;
            self.reviews = reviews::load_cached(conn);
            self.review_total = reviews::load_cached_total(conn);
            self.review_hits = reviews::load_cached_hits(conn).unwrap_or_default();
            self.people = reviews::load_cached_people(conn).unwrap_or_default();
        }
    }

    /// People who can author PRs in the tracked repos (from the last lookup
    /// or the cache), for `reviewPriorityAuthors` autocomplete.
    pub fn people(&self) -> &[ReviewPerson] {
        &self.people
    }

    /// Look up `assignableUsers` for every tracked repo on GitHub, at most
    /// once per [`PEOPLE_TTL`]. Autocomplete is optional, so a failure is
    /// only logged: the breaker isn't tripped and the cached list stays.
    pub fn refresh_people(&mut self, conn: &Connection, repos: &[reviews::ReviewRepo]) {
        self.hydrate(conn);
        let now = Instant::now();
        if self.people_at.is_some_and(|t| now.duration_since(t) < PEOPLE_TTL) || !self.github_breaker.allow(now, false) {
            return;
        }
        let mut slugs = Vec::new();
        let mut cwd = None;
        for repo in repos {
            if !self.slugs.contains_key(&repo.path) {
                match reviews::repo_slug(self.gh.as_ref(), &repo.path) {
                    Ok(slug) => {
                        self.slugs.insert(repo.path.clone(), slug);
                    }
                    Err((e, _)) => debug!("reviews", "No people lookup for {}: {e}", repo.path.display()),
                }
            }
            if let Some(slug) = self.slugs.get(&repo.path) {
                slugs.push(slug.clone());
                cwd.get_or_insert_with(|| repo.path.clone());
            }
        }
        let Some(cwd) = cwd else { return };
        self.people_at = Some(now);
        let started = Instant::now();
        match reviews::fetch_people(self.gh.as_ref(), &cwd, &slugs) {
            Ok(people) => {
                debug!("reviews", "{} people in {} repo(s) in {:?}", people.len(), slugs.len(), started.elapsed());
                reviews::store_people(conn, &people);
                self.people = people;
            }
            Err(e) => debug!("reviews", "People lookup failed (autocomplete keeps {} cached): {e}", self.people.len()),
        }
    }

    /// The review requests from the last successful fetch (or the cache).
    pub fn reviews(&self) -> Option<&[ReviewRequest]> {
        self.reviews.as_deref()
    }

    /// The unfiltered review count (badge `all`), from the last search that
    /// asked for it or the cache.
    pub fn review_total(&self) -> Option<u64> {
        self.review_total
    }

    /// Which PRs match each review group's GitHub search terms.
    pub fn review_hits(&self) -> &crate::review_groups::QueryHits {
        &self.review_hits
    }

    /// Why the last review search failed (rate limit included), if it did.
    pub fn review_error(&self) -> Option<&ReviewFetchError> {
        self.review_error.as_ref()
    }

    /// Forget the review list (memory + disk), e.g. the feature was turned off.
    pub fn clear_reviews(&mut self, conn: &Connection) {
        self.reviews = None;
        self.review_error = None;
        self.review_total = None;
        self.reviews_hydrated = true;
        let _ = cache::prune_rows(conn, cache::CacheKind::Review, &[]);
    }

    /// Refresh the PRs waiting on your review in `repos` (see [`reviews`]):
    /// one `gh repo view` per repo the first time, then one search. Shares the
    /// GitHub breaker; a failure keeps the previous list. Tickets come from
    /// Linear by head branch when `settings` has a Linear key, and a failed
    /// ticket lookup keeps each PR's previous ticket.
    pub fn refresh_reviews(
        &mut self,
        conn: &Connection,
        repos: &[reviews::ReviewRepo],
        settings: &Settings,
        force: bool,
    ) -> RefreshOutcome {
        self.hydrate(conn);
        if repos.is_empty() {
            // Nothing tracked: nothing waits (rather than "loading" forever).
            self.review_error = None;
            if self.reviews.is_none() {
                self.reviews = Some(Vec::new());
            }
            return RefreshOutcome::Idle;
        }
        let now = Instant::now();
        if !self.github_breaker.allow(now, force) {
            // GitHub is backing off (maybe after the PR fetch hit a rate
            // limit): say why instead of leaving the page on "Loading…".
            if self.review_error.is_none()
                && let Some(e) = self.github_breaker.last_error().map(str::to_string)
            {
                self.review_error = Some(reviews::fetch_error(self.gh.as_ref(), &repos[0].path, &e));
            }
            return RefreshOutcome::SkippedBackoff;
        }
        let mut repo_ids: HashMap<String, String> = HashMap::new();
        let mut slugs: Vec<String> = Vec::new();
        let mut cwd: Option<PathBuf> = None;
        let mut slug_error: Option<String> = None;
        for repo in repos {
            if !self.slugs.contains_key(&repo.path) {
                match reviews::repo_slug(self.gh.as_ref(), &repo.path) {
                    Ok(slug) => {
                        debug!("reviews", "{} is {slug} on GitHub", repo.path.display());
                        self.slugs.insert(repo.path.clone(), slug);
                    }
                    Err((e, true)) => {
                        finish(&mut self.github_breaker, now, Some(&e), None);
                        self.review_error = Some(reviews::fetch_error(self.gh.as_ref(), &repo.path, &e));
                        return RefreshOutcome::Ran { fetched: 0, failed: 1, aborted: true };
                    }
                    Err((e, false)) => {
                        debug!("reviews", "{} has no GitHub repo ({e}); left out of review requests", repo.path.display());
                        slug_error.get_or_insert(e);
                    }
                }
            }
            let Some(slug) = self.slugs.get(&repo.path) else { continue };
            repo_ids.insert(slug.to_lowercase(), repo.repo_id.clone());
            slugs.push(slug.clone());
            cwd.get_or_insert_with(|| repo.path.clone());
        }
        let Some(cwd) = cwd else {
            if let Some(e) = slug_error.filter(|_| self.slugs.is_empty()) {
                // No repo resolved at all (not on GitHub, or gh isn't logged
                // in): show gh's reason rather than an empty list.
                self.review_error = Some(reviews::fetch_error(self.gh.as_ref(), &repos[0].path, &e));
                return RefreshOutcome::Ran { fetched: 0, failed: 1, aborted: false };
            }
            // Every repo filtered out: an empty list is the true answer.
            self.set_reviews(conn, Vec::new());
            self.review_error = None;
            return RefreshOutcome::Ran { fetched: 0, failed: 0, aborted: false };
        };
        let search = reviews::build_search(settings, &slugs);
        debug!("reviews", "Searching review requests: {search}");
        let queries = crate::review_groups::queries(&settings.review_groups, &settings.review_default_group);
        let extras = reviews::Extras { total: settings.review_badge_total, queries: &queries };
        match reviews::fetch(self.gh.as_ref(), &cwd, &search, &repo_ids, extras) {
            Ok(found) => {
                if !queries.is_empty() || !self.review_hits.is_empty() {
                    reviews::store_hits(conn, &found.hits);
                }
                self.review_hits = found.hits;
                if let Some(total) = found.total {
                    if self.review_total != Some(total) {
                        debug!("reviews", "{total} PR(s) request your or your teams' review on GitHub (unfiltered)");
                        reviews::store_total(conn, total);
                    }
                    self.review_total = Some(total);
                }
                let mut list = found.prs;
                self.fill_tickets(settings, &mut list, force);
                let n = list.len();
                if self.reviews.as_ref().map(Vec::len) != Some(n) {
                    info!("reviews", "{n} PR(s) waiting on your review");
                }
                self.set_reviews(conn, list);
                self.review_error = None;
                finish(&mut self.github_breaker, now, None, None);
                RefreshOutcome::Ran { fetched: n, failed: 0, aborted: false }
            }
            Err((e, network)) => {
                debug!("reviews", "Review request search failed: {e}");
                finish(&mut self.github_breaker, now, network.then_some(e.as_str()), Some(&e));
                let err = reviews::fetch_error(self.gh.as_ref(), &cwd, &e);
                if err.rate_limited {
                    info!("reviews", "GitHub rate limit hit; resets at {:?} (unix)", err.resets_at);
                }
                self.review_error = Some(err);
                RefreshOutcome::Ran { fetched: 0, failed: 1, aborted: network }
            }
        }
    }

    fn set_reviews(&mut self, conn: &Connection, list: Vec<ReviewRequest>) {
        reviews::store(conn, &list);
        self.reviews = Some(list);
    }

    /// Linear tickets for the review list's head branches (workspace-wide
    /// search, so another repo's branch works). Keeps the previous ticket of a
    /// PR whose lookup failed.
    fn fill_tickets(&mut self, settings: &Settings, list: &mut [ReviewRequest], force: bool) {
        let previous: HashMap<&str, &LinearInfo> = self
            .reviews
            .iter()
            .flatten()
            .filter_map(|r| r.linear.as_ref().map(|l| (r.pr.url.as_str(), l)))
            .collect();
        let key = settings.linear_api_key.trim();
        let ask = settings.linear_enabled && !key.is_empty() && self.linear_breaker.allow(Instant::now(), force);
        let batch = ask.then(|| {
            let heads = distinct(list.iter().map(|r| &r.head_ref_name));
            linear::fetch_linear_batch(self.http.as_ref(), &self.linear_url, key, &heads)
        });
        for r in list.iter_mut() {
            let fetched = batch.as_ref().and_then(|b| b.results.iter().find(|(h, _)| *h == r.head_ref_name));
            r.linear = match fetched {
                Some((_, FetchResult::Ok(t))) => t.clone(),
                _ if settings.linear_enabled => previous.get(r.pr.url.as_str()).map(|l| (*l).clone()),
                _ => None,
            };
        }
    }

    /// Refresh PR info for every branch of every repo: one `gh api graphql`
    /// per [`github::GRAPHQL_CHUNK`] branches of each repo, a few at a time.
    /// Branches whose fetch failed keep their cached PR. `force` (user
    /// refresh) bypasses the backoff.
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

        let (mut fetched, mut failed) = (0, 0);
        let mut first_error: Option<String> = None;
        let mut network_error: Option<String> = None;
        // Fetch everything first, then write in one short transaction: holding a
        // write lock across seconds of `gh` calls would stall the hook writers.
        let mut successes: Vec<(String, FetchResult<PrInfo>)> = Vec::new();
        for repo in repos.iter().filter(|r| !r.branches.is_empty()) {
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
        }
        let tx = conn.unchecked_transaction().ok();
        for (key, result) in &successes {
            self.cache.commit_pr(conn, key, result);
        }
        if let Some(tx) = tx {
            let _ = tx.commit();
        }
        finish(&mut self.github_breaker, now, network_error.as_deref(), first_error.as_deref());
        RefreshOutcome::Ran { fetched, failed, aborted: network_error.is_some() }
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
    pub fn fields_for(&self, repo_id: &str, branch: &str) -> (Option<PrInfo>, Option<LinearInfo>) {
        self.cache.fields_for(repo_id, branch)
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

    fn review_body(heads: &[&str]) -> String {
        let nodes: Vec<Value> = heads
            .iter()
            .enumerate()
            .map(|(i, h)| reviews::tests::search_node(i as u64 + 1, "acme/web", h, vec![json!({"login": "me"})]))
            .collect();
        json!({"data": {"search": {"nodes": nodes}}}).to_string()
    }

    #[test]
    fn reviews_search_once_keep_the_list_offline_and_fill_tickets() {
        let mut e = env();
        let repos = vec![reviews::ReviewRepo::new("r1", "web", "/a")];
        let s = Settings::parse(r#"{"linearEnabled":true,"linearApiKey":"key"}"#);
        e.gh.push(GhOutput::success("acme/web\n"));
        e.gh.push(GhOutput::success(review_body(&["feature/x", "feature/y"])));
        e.http.push(ok(aliased(vec![issue("ENG-1"), Value::Null])));
        e.ints.refresh_reviews(&e.conn, &repos, &s, false);
        let list = e.ints.reviews().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].linear.as_ref().unwrap().identifier, "ENG-1");
        assert!(list[1].linear.is_none());
        let calls = e.gh.calls.lock().unwrap().clone();
        assert!(calls[1][3].contains("repo:acme/web"), "{:?}", calls[1]);

        // Offline: the slug is remembered, the list and its tickets are kept.
        e.gh.push(GhOutput::failure("error connecting to api.github.com"));
        e.ints.refresh_reviews(&e.conn, &repos, &s, false);
        assert_eq!(e.gh.call_count(), 3);
        assert_eq!(e.ints.reviews().unwrap().len(), 2);
        assert!(e.ints.health().github_failing);

        // A fresh process hydrates the cached list.
        let mut fresh = Integrations::with_transports(Box::new(FakeGh::default()), Box::new(FakeHttp::default()), "x".into());
        fresh.hydrate(&e.conn);
        assert_eq!(fresh.reviews().unwrap().len(), 2);

        // Linear down: the previous ticket stays.
        e.gh.push(GhOutput::success(review_body(&["feature/x"])));
        e.http.push(offline());
        e.ints.refresh_reviews(&e.conn, &repos, &s, true);
        assert_eq!(e.ints.reviews().unwrap()[0].linear.as_ref().unwrap().identifier, "ENG-1");
    }

    #[test]
    fn a_rate_limited_review_search_reports_why_and_keeps_the_list() {
        let mut e = env();
        let repos = vec![reviews::ReviewRepo::new("r1", "web", "/a")];
        let s = Settings::default();
        let limited = r#"{"errors":[{"type":"RATE_LIMITED","message":"API rate limit exceeded for user ID 1."}]}"#;
        e.gh.push(GhOutput { ok: false, stdout: limited.into(), stderr: String::new() }); // repo view
        e.gh.push(GhOutput::success("1793016000"));
        e.ints.refresh_reviews(&e.conn, &repos, &s, false);
        let err = e.ints.review_error().unwrap().clone();
        assert!(err.rate_limited && err.resets_at == Some(1_793_016_000));
        assert!(e.ints.reviews().is_none(), "never fetched: the page shows the error, not a list");

        // Backing off: no new calls, the error stays.
        e.ints.refresh_reviews(&e.conn, &repos, &s, false);
        assert_eq!(e.gh.call_count(), 2);

        // `r` gets through; a success clears it.
        e.gh.push(GhOutput::success("acme/web"));
        e.gh.push(GhOutput::success(review_body(&["feature/x"])));
        e.ints.refresh_reviews(&e.conn, &repos, &s, true);
        assert!(e.ints.review_error().is_none());
        assert_eq!(e.ints.reviews().unwrap().len(), 1);

        // Limited again with a list: the list stays beside the error.
        e.gh.push(GhOutput { ok: false, stdout: limited.into(), stderr: String::new() });
        e.gh.push(GhOutput::failure("rate_limit lookup failed"));
        e.ints.refresh_reviews(&e.conn, &repos, &s, true);
        assert_eq!(e.ints.reviews().unwrap().len(), 1);
        assert_eq!(e.ints.review_error().unwrap().resets_at, None);
    }

    #[test]
    fn a_breaker_opened_by_the_pr_fetch_explains_the_empty_review_page() {
        let mut e = env();
        e.gh.push(GhOutput::failure("gh: API rate limit exceeded for user ID 1."));
        e.ints.refresh_github(&e.conn, &[RepoBranches::new("r1", "/a", vec!["main".into()])], false);
        e.gh.push(GhOutput::success("1793016000"));
        e.ints.refresh_reviews(&e.conn, &[reviews::ReviewRepo::new("r1", "web", "/a")], &Settings::default(), false);
        assert!(e.ints.review_error().unwrap().rate_limited);
    }

    #[test]
    fn reviews_skip_excluded_and_non_github_repos() {
        let mut e = env();
        let repos = vec![reviews::ReviewRepo::new("r1", "web", "/a"), reviews::ReviewRepo::new("r2", "local", "/b")];
        // Excluding repos is a review group's job now: the search covers them.
        let s = Settings::parse(r#"{"reviewExcludeRepos":"web"}"#);
        e.gh.push(GhOutput::success("acme/web"));
        e.gh.push(GhOutput::failure("none of the git remotes configured for this repository point to a known GitHub host"));
        e.gh.push(GhOutput::success(review_body(&[])));
        e.ints.refresh_reviews(&e.conn, &repos, &s, false);
        assert_eq!(e.gh.call_count(), 3);
        assert!(e.gh.calls.lock().unwrap()[2][3].contains("repo:acme/web"));
        assert_eq!(e.ints.reviews(), Some(&[][..]));
        assert!(e.ints.review_error().is_none());

        // Nothing resolves (gh logged out): say so, and ask again next time.
        let mut e = env();
        let repos = vec![reviews::ReviewRepo::new("r1", "web", "/a")];
        e.gh.push(GhOutput::failure("To get started with GitHub CLI, please run:  gh auth login"));
        e.ints.refresh_reviews(&e.conn, &repos, &Settings::default(), false);
        assert!(e.ints.review_error().unwrap().message.contains("gh auth login"));
        assert!(e.ints.reviews().is_none());
        e.gh.push(GhOutput::success("acme/web"));
        e.gh.push(GhOutput::success(review_body(&[])));
        e.ints.refresh_reviews(&e.conn, &repos, &Settings::default(), false);
        assert!(e.ints.review_error().is_none());
        assert_eq!(e.ints.reviews(), Some(&[][..]));
    }

    #[test]
    fn people_are_looked_up_once_an_hour_and_cached() {
        let mut e = env();
        let repos = vec![reviews::ReviewRepo::new("r1", "web", "/a")];
        e.gh.push(GhOutput::success("acme/web"));
        let body = json!({"data": {"r0": {"assignableUsers": {"nodes": [{"login": "alice", "name": "Alice"}]}}}});
        e.gh.push(GhOutput::success(body.to_string()));
        e.ints.refresh_people(&e.conn, &repos);
        assert_eq!(e.ints.people().len(), 1);
        e.ints.refresh_people(&e.conn, &repos);
        assert_eq!(e.gh.call_count(), 2, "memoised");

        // A fresh process starts from the cache; a failed lookup keeps it.
        e.restart();
        e.gh.push(GhOutput::success("acme/web"));
        e.gh.push(gh_offline());
        e.ints.refresh_people(&e.conn, &repos);
        assert_eq!(e.ints.people()[0].login, "alice");
        assert!(!e.ints.health().github_failing, "autocomplete never trips the breaker");
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
