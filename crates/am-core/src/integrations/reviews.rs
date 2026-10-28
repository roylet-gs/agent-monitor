//! Open PRs waiting on your review, in the tracked repos.
//!
//! One `gh api graphql` `search(type: ISSUE)` call per refresh
//! ([`build_search`]): `review-requested:@me` (you or a team you're on)
//! narrowed by `reviewRequestsScope`, with one `repo:` per tracked repo. The
//! rest of the filtering (drafts, reviewed, repos…) is the review groups'
//! (`crate::review_groups`), done locally; a group's GitHub search terms
//! ride along in the first page's request as aliased URL-only searches
//! ([`Extras`]), so the groups know which PRs match them. Each tracked repo's `owner/name` comes from
//! `gh repo view` in its checkout, so gh's own remote rules apply (and the
//! E2E shim can answer for a local bare origin).
//!
//! The list is cached as a single `integration_cache` row (`kind = "review"`,
//! `cache_key = "requests"`) and, like PRs and tickets, only a successful
//! fetch replaces it, so it survives going offline.
//!
//! When the badge asks for it (`reviewBadgeTotal`), the first page's
//! request also counts every open PR requesting your or your teams' review on
//! GitHub, ignoring the filters ([`TOTAL_SEARCH`], `issueCount` only), so the
//! unfiltered number costs no extra request. Cached under key `total`.
//!
//! A PR you approve stops requesting your review, so it leaves the search.
//! While a tmp-space worktree (Reviewing) still has its branch, the PR is
//! looked up by branch ([`pulled_args`]) and kept, marked `approved_only`, if
//! your latest review approved it, so the page can remind you to remove it.
//!
//! [`people_args`] looks up who can author PRs in the tracked repos (each
//! repo's `assignableUsers`), the autocomplete for `reviewPriorityAuthors`.
//! Cached the same way under `kind = "people"`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::Connection;
use serde_json::Value;

use super::breaker::{is_network_error, is_rate_limit_error, is_timeout_error, summarize_error};
use super::cache::{CacheKind, load_rows, set_entry};
use super::github::{GhRunner, pr_info_from_node, pr_selection_counts};
use crate::model::{ReviewFetchError, ReviewPerson, ReviewRequest};
use crate::review_groups::QueryHits;
use crate::settings::{ReviewScope, Settings};
use crate::{debug, warn};

pub const SEARCH_TIMEOUT: Duration = Duration::from_secs(20);
pub const SLUG_TIMEOUT: Duration = Duration::from_secs(10);
/// PRs per search request. Each one is a separate request against GitHub's
/// 10 s limit, so a long list is fetched in a few lighter pages.
pub const PAGE_SIZE: usize = 50;
/// Pages fetched at most: more review requests than `PAGE_SIZE * MAX_PAGES`
/// at once only shows the newest of them.
pub const MAX_PAGES: usize = 2;
const CACHE_KEY: &str = "requests";
const TOTAL_KEY: &str = "total";
const QUERIES_KEY: &str = "queries";
/// PRs a group's GitHub search terms are checked against (newest first).
pub const QUERY_SIZE: usize = 100;
/// The unfiltered count's search: every open PR waiting on you or a team of
/// yours, anywhere on GitHub.
pub const TOTAL_SEARCH: &str = "is:pr is:open archived:false review-requested:@me";
const PEOPLE_KEY: &str = "assignable";
/// People per repo asked for (one page; enough for autocomplete).
pub const PEOPLE_PER_REPO: usize = 100;

/// A tracked repo the search may cover.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewRepo {
    pub repo_id: String,
    /// The repository's display name (its directory name).
    pub name: String,
    pub path: PathBuf,
    /// Branches pulled into the tmp space (Reviewing) from this repo.
    pub pulled: Vec<String>,
}

impl ReviewRepo {
    pub fn new(repo_id: impl Into<String>, name: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        ReviewRepo { repo_id: repo_id.into(), name: name.into(), path: path.into(), pulled: Vec::new() }
    }

    pub fn with_pulled(mut self, pulled: Vec<String>) -> Self {
        self.pulled = pulled;
        self
    }
}

/// `owner/name` of the GitHub repo checked out at `path`. `Err((message,
/// network))`.
pub fn repo_slug(gh: &dyn GhRunner, path: &Path) -> Result<String, (String, bool)> {
    let args: Vec<String> =
        ["repo", "view", "--json", "nameWithOwner", "-q", ".nameWithOwner"].iter().map(|s| s.to_string()).collect();
    let out = gh.run(path, &args, SLUG_TIMEOUT);
    let slug = out.stdout.trim();
    if out.ok && slug.contains('/') {
        return Ok(slug.to_string());
    }
    let err = match (out.ok, out.stderr.trim().is_empty()) {
        (true, _) => format!("unexpected gh repo view output: {slug}"),
        (false, true) => out.stdout.clone(),
        (false, false) => out.stderr.clone(),
    };
    Err((summarize_error(&err), !out.ok && is_network_error(&err)))
}

/// The GitHub search string for the tracked repos' `slugs`.
pub fn build_search(settings: &Settings, slugs: &[String]) -> String {
    let mut q = vec!["is:pr".to_string(), "is:open".into(), "archived:false".into()];
    q.push(
        match settings.review_scope() {
            ReviewScope::All => "review-requested:@me",
            ReviewScope::Direct => "user-review-requested:@me",
            ReviewScope::Teams => "team-review-requested-user:@me",
        }
        .into(),
    );
    q.extend(slugs.iter().map(|s| format!("repo:{s}")));
    q.join(" ")
}

/// What the first page's request asks for besides the list: the unfiltered
/// count (`total`) and, per group query, which PRs match it.
#[derive(Debug, Clone, Copy, Default)]
pub struct Extras<'a> {
    pub total: bool,
    /// Group queries ([`crate::review_groups::queries`]).
    pub queries: &'a [String],
}

/// What a review row needs of a `PullRequest`: [`pr_selection_counts`] plus
/// dates, author, repo, your latest review and the pending reviewers.
fn review_fields() -> String {
    format!(
        "{} createdAt updatedAt author {{ login }} repository {{ nameWithOwner }} viewerLatestReview {{ state }} \
         reviewRequests(first: 20) {{ nodes {{ requestedReviewer {{ ... on User {{ login }} \
         ... on Team {{ combinedSlug }} ... on Bot {{ login }} ... on Mannequin {{ login }} }} }} }} }}",
        pr_selection_counts()
    )
}

/// `gh api graphql` argv for one page of a search. The search string (and
/// the page cursor) travel as variables; the query text is a single line (the
/// E2E shim splits argv on newlines). Checks are counts only
/// ([`pr_selection_counts`]), which keeps a page well inside GitHub's 10 s.
/// `extras` adds a count-only search (`total { issueCount }`) and one
/// URL-only search per group query (`gN`, the list's search plus the terms).
pub fn search_args(search: &str, after: Option<&str>, extras: Extras) -> Vec<String> {
    let mut vars = String::new();
    let mut fields = String::new();
    let mut values: Vec<String> = Vec::new();
    if extras.total {
        vars.push_str(", $all: String!");
        fields.push_str("total: search(query: $all, type: ISSUE, first: 1) { issueCount } ");
        values.push(format!("all={TOTAL_SEARCH}"));
    }
    for (i, q) in extras.queries.iter().enumerate() {
        vars.push_str(&format!(", $g{i}: String!"));
        fields.push_str(&format!(
            "g{i}: search(query: $g{i}, type: ISSUE, first: {QUERY_SIZE}) {{ nodes {{ ... on PullRequest {{ url }} }} }} "
        ));
        values.push(format!("g{i}={search} {q}"));
    }
    let query = format!(
        "query($q: String!, $after: String{vars}) {{ viewer {{ login }} {fields}\
         search(query: $q, type: ISSUE, first: {PAGE_SIZE}, after: $after) {{ pageInfo {{ hasNextPage endCursor }} nodes {{ \
         ... on PullRequest {{ {} }} }} }} }}",
        review_fields()
    );
    let mut args: Vec<String> = ["api", "graphql", "-f"].iter().map(|s| s.to_string()).collect();
    args.push(format!("q={search}"));
    if let Some(cursor) = after {
        args.extend(["-f".into(), format!("after={cursor}")]);
    }
    for v in values {
        args.extend(["-f".into(), v]);
    }
    args.extend(["-f".into(), format!("query={query}")]);
    args
}

/// One parsed page: its PRs, the cursor of the next page, if any, and the
/// unfiltered count when it was asked for.
#[derive(Debug)]
pub struct SearchPage {
    pub prs: Vec<ReviewRequest>,
    pub next: Option<String>,
    pub total: Option<u64>,
    /// Per group query asked for, the matching PRs' urls.
    pub hits: QueryHits,
}

/// A whole search: every page's PRs, the unfiltered count and group query
/// hits if asked for.
#[derive(Debug, Default)]
pub struct SearchResult {
    pub prs: Vec<ReviewRequest>,
    pub total: Option<u64>,
    pub hits: QueryHits,
}

/// Parse one page of a search response. `repo_ids` maps lower-cased
/// `owner/name` to the tracked repo id; PRs in any other repo are dropped. A
/// pending reviewer who is the viewer reads `you`. `queries` are the group
/// queries the request asked for (`gN`), in order.
pub fn parse_search(stdout: &str, repo_ids: &HashMap<String, String>, queries: &[String]) -> Result<SearchPage, String> {
    let json: Value = serde_json::from_str(stdout.trim()).map_err(|e| format!("unparseable gh graphql output: {e}"))?;
    let nodes = json.pointer("/data/search/nodes").and_then(Value::as_array).ok_or_else(|| {
        let msg = json.pointer("/errors/0/message").and_then(Value::as_str).unwrap_or("no search in response");
        format!("graphql: {msg}")
    })?;
    let str_at = |n: &Value, p: &str| n.pointer(p).and_then(Value::as_str).unwrap_or("").to_string();
    let viewer = str_at(&json, "/data/viewer/login");
    let mut out = Vec::new();
    for node in nodes {
        let repo = str_at(node, "/repository/nameWithOwner");
        let Some(repo_id) = repo_ids.get(&repo.to_lowercase()) else { continue };
        out.extend(review_from_node(node, repo_id, &repo, &viewer));
    }
    let next = (json.pointer("/data/search/pageInfo/hasNextPage").and_then(Value::as_bool) == Some(true))
        .then(|| str_at(&json, "/data/search/pageInfo/endCursor"))
        .filter(|c| !c.is_empty());
    let total = json.pointer("/data/total/issueCount").and_then(Value::as_u64);
    let mut hits = QueryHits::new();
    for (i, q) in queries.iter().enumerate() {
        let Some(nodes) = json.pointer(&format!("/data/g{i}/nodes")).and_then(Value::as_array) else { continue };
        hits.insert(q.clone(), nodes.iter().map(|n| str_at(n, "/url")).filter(|u| !u.is_empty()).collect());
    }
    Ok(SearchPage { prs: out, next, total, hits })
}

/// A [`ReviewRequest`] from one PR node (search or `pullRequests`) of
/// tracked repo `repo_id` (`owner/name` `repo`). A pending reviewer who is
/// `viewer` reads `you`. `None` for a node that isn't a PR.
fn review_from_node(node: &Value, repo_id: &str, repo: &str, viewer: &str) -> Option<ReviewRequest> {
    let str_at = |p: &str| node.pointer(p).and_then(Value::as_str).unwrap_or("").to_string();
    let pr = pr_info_from_node(node)?;
    let requested = node
        .pointer("/reviewRequests/nodes")
        .and_then(Value::as_array)
        .map(|rs| {
            rs.iter()
                .filter_map(|r| {
                    let who = r.get("requestedReviewer")?;
                    let name = who.get("login").or_else(|| who.get("combinedSlug")).and_then(Value::as_str)?;
                    Some(if !viewer.is_empty() && name == viewer { "you".to_string() } else { name.to_string() })
                })
                .collect()
        })
        .unwrap_or_default();
    let latest = node.pointer("/viewerLatestReview/state").and_then(Value::as_str);
    Some(ReviewRequest {
        repo_id: repo_id.to_string(),
        repo: repo.to_string(),
        pr,
        author: str_at("/author/login"),
        head_ref_name: str_at("/headRefName"),
        is_cross_repository: node.get("isCrossRepository").and_then(Value::as_bool).unwrap_or(false),
        created_at: str_at("/createdAt"),
        updated_at: str_at("/updatedAt"),
        requested,
        linear: None,
        reviewed_by_you: latest.is_some(),
        approved_by_you: latest == Some("APPROVED"),
        approved_only: false,
        worktree_id: None,
    })
}

/// Run a search, page by page (at most [`MAX_PAGES`]), with the `extras`
/// on the first page. `Err((message, network))`; a
/// failed page fails the whole search, so the cached list stays rather than
/// being replaced by part of it.
pub fn fetch(
    gh: &dyn GhRunner,
    cwd: &Path,
    search: &str,
    repo_ids: &HashMap<String, String>,
    extras: Extras,
) -> Result<SearchResult, (String, bool)> {
    let mut all = SearchResult::default();
    let mut after: Option<String> = None;
    for page in 0..MAX_PAGES {
        let extras = if page == 0 { extras } else { Extras::default() };
        let out = gh.run(cwd, &search_args(search, after.as_deref(), extras), SEARCH_TIMEOUT);
        // gh exits 1 on GraphQL errors but still prints the JSON body; on an
        // HTTP error (a 504 when GitHub's 10 s ran out) stdout is GitHub's
        // HTML error page and stderr says what happened.
        if !out.stdout.trim_start().starts_with('{') {
            let err = if out.ok { "unexpected gh graphql output".to_string() } else { out.stderr.clone() };
            return Err((summarize_error(&err), !out.ok && is_network_error(&err)));
        }
        // A rate limit comes back as a GraphQL error in the body.
        let parsed =
            parse_search(&out.stdout, repo_ids, extras.queries).map_err(|e| (summarize_error(&e), is_network_error(&e)))?;
        all.prs.extend(parsed.prs);
        all.total = all.total.or(parsed.total);
        all.hits.extend(parsed.hits);
        match parsed.next {
            Some(cursor) if page + 1 < MAX_PAGES => after = Some(cursor),
            Some(_) => {
                debug!("reviews", "More than {} review requests; showing the newest", PAGE_SIZE * MAX_PAGES);
                break;
            }
            None => break,
        }
    }
    Ok(all)
}

/// A pulled branch to look up: its tracked repo, `owner/name` and branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pulled {
    pub repo_id: String,
    pub slug: String,
    pub branch: String,
}

/// `gh api graphql` argv for the newest PR (any state) of each `pulled`
/// branch: one aliased `repository { pullRequests(headRefName:) }` per
/// branch, owner, name and branch as variables, on one line (the E2E shim
/// splits argv on newlines).
pub fn pulled_args(pulled: &[Pulled]) -> Vec<String> {
    let mut vars = Vec::new();
    let mut fields = Vec::new();
    let mut args: Vec<String> = vec!["api".into(), "graphql".into()];
    for (i, p) in pulled.iter().enumerate() {
        let Some((owner, name)) = p.slug.split_once('/') else { continue };
        vars.push(format!("$o{i}: String!, $n{i}: String!, $b{i}: String!"));
        fields.push(format!(
            "p{i}: repository(owner: $o{i}, name: $n{i}) {{ pullRequests(headRefName: $b{i}, first: 1, \
             orderBy: {{ field: UPDATED_AT, direction: DESC }}) {{ nodes {{ {} }} }} }}",
            review_fields()
        ));
        args.extend(["-f".into(), format!("o{i}={owner}"), "-f".into(), format!("n{i}={name}")]);
        args.extend(["-f".into(), format!("b{i}={}", p.branch)]);
    }
    args.extend(["-f".into(), format!("query=query({}) {{ viewer {{ login }} {} }}", vars.join(", "), fields.join(" "))]);
    args
}

/// The PRs of a [`pulled_args`] response that you approved, marked
/// `approved_only`. A branch without a PR (or a repo that errored) is
/// skipped as long as some data came back.
pub fn parse_pulled(stdout: &str, pulled: &[Pulled]) -> Result<Vec<ReviewRequest>, String> {
    let json: Value = serde_json::from_str(stdout.trim()).map_err(|e| format!("unparseable gh graphql output: {e}"))?;
    if json.get("data").and_then(Value::as_object).is_none() {
        let msg = json.pointer("/errors/0/message").and_then(Value::as_str).unwrap_or("no data in response");
        return Err(format!("graphql: {msg}"));
    }
    let viewer = json.pointer("/data/viewer/login").and_then(Value::as_str).unwrap_or("");
    let mut out = Vec::new();
    for (i, p) in pulled.iter().enumerate() {
        let Some(node) = json.pointer(&format!("/data/p{i}/pullRequests/nodes/0")) else { continue };
        let Some(mut r) = review_from_node(node, &p.repo_id, &p.slug, viewer) else { continue };
        if r.approved_by_you && !r.is_cross_repository {
            r.approved_only = true;
            out.push(r);
        }
    }
    Ok(out)
}

/// Run [`pulled_args`] for `pulled`. `Err` is one line, for the debug log.
pub fn fetch_pulled(gh: &dyn GhRunner, cwd: &Path, pulled: &[Pulled]) -> Result<Vec<ReviewRequest>, String> {
    let out = gh.run(cwd, &pulled_args(pulled), SEARCH_TIMEOUT);
    if !out.stdout.trim_start().starts_with('{') {
        return Err(summarize_error(if out.ok { "unexpected gh graphql output" } else { &out.stderr }));
    }
    parse_pulled(&out.stdout, pulled).map_err(|e| summarize_error(&e))
}

/// `gh api graphql` argv listing who can be assigned in each of `slugs`
/// (`owner/name`): one aliased `repository` per repo, owner and name as
/// variables, the query on one line (the E2E shim splits argv on newlines).
pub fn people_args(slugs: &[String]) -> Vec<String> {
    let mut vars = Vec::new();
    let mut fields = Vec::new();
    let mut args: Vec<String> = vec!["api".into(), "graphql".into()];
    for (i, slug) in slugs.iter().enumerate() {
        let Some((owner, name)) = slug.split_once('/') else { continue };
        vars.push(format!("$o{i}: String!, $n{i}: String!"));
        fields.push(format!(
            "r{i}: repository(owner: $o{i}, name: $n{i}) {{ assignableUsers(first: {PEOPLE_PER_REPO}) {{ nodes {{ login name }} }} }}"
        ));
        args.extend(["-f".into(), format!("o{i}={owner}"), "-f".into(), format!("n{i}={name}")]);
    }
    args.extend(["-f".into(), format!("query=query({}) {{ {} }}", vars.join(", "), fields.join(" "))]);
    args
}

/// Every repo's assignable users from a [`people_args`] response, one entry
/// per login (case-insensitive), sorted by login. A repo that errored (say,
/// renamed) is skipped as long as some data came back.
pub fn parse_people(stdout: &str) -> Result<Vec<ReviewPerson>, String> {
    let json: Value = serde_json::from_str(stdout.trim()).map_err(|e| format!("unparseable gh graphql output: {e}"))?;
    let Some(data) = json.get("data").and_then(Value::as_object) else {
        let msg = json.pointer("/errors/0/message").and_then(Value::as_str).unwrap_or("no data in response");
        return Err(format!("graphql: {msg}"));
    };
    let mut out: Vec<ReviewPerson> = Vec::new();
    for repo in data.values() {
        for node in repo.pointer("/assignableUsers/nodes").and_then(Value::as_array).into_iter().flatten() {
            let Some(login) = node.get("login").and_then(Value::as_str).filter(|l| !l.is_empty()) else { continue };
            if out.iter().any(|p| p.login.eq_ignore_ascii_case(login)) {
                continue;
            }
            let name = node.get("name").and_then(Value::as_str).unwrap_or("").to_string();
            out.push(ReviewPerson { login: login.to_string(), name });
        }
    }
    out.sort_by_key(|p| p.login.to_lowercase());
    Ok(out)
}

/// Run [`people_args`] for `slugs`. `Err` is one line, for the debug log.
pub fn fetch_people(gh: &dyn GhRunner, cwd: &Path, slugs: &[String]) -> Result<Vec<ReviewPerson>, String> {
    let out = gh.run(cwd, &people_args(slugs), SEARCH_TIMEOUT);
    if !out.stdout.trim_start().starts_with('{') {
        return Err(summarize_error(if out.ok { "unexpected gh graphql output" } else { &out.stderr }));
    }
    parse_people(&out.stdout).map_err(|e| summarize_error(&e))
}

/// The cached people list, if a lookup ever succeeded.
pub fn load_cached_people(conn: &Connection) -> Option<Vec<ReviewPerson>> {
    let rows = load_rows(conn, CacheKind::People).ok()?;
    let payload = rows.into_iter().find(|r| r.cache_key == PEOPLE_KEY)?.payload?;
    serde_json::from_str(&payload).ok()
}

/// Persist a successfully fetched people list.
pub fn store_people(conn: &Connection, people: &[ReviewPerson]) {
    let payload = serde_json::to_string(people).ok();
    if let Err(e) = set_entry(conn, CacheKind::People, PEOPLE_KEY, payload.as_deref()) {
        warn!("integration-cache", "Failed to persist review people: {e}");
    }
}

/// When GitHub's GraphQL rate limit resets (unix seconds). `gh api
/// rate_limit` doesn't count against the limit, so it still answers while
/// limited.
pub fn rate_limit_reset(gh: &dyn GhRunner, cwd: &Path) -> Option<i64> {
    let args: Vec<String> =
        ["api", "rate_limit", "--jq", ".resources.graphql.reset"].iter().map(|s| s.to_string()).collect();
    let out = gh.run(cwd, &args, SLUG_TIMEOUT);
    if !out.ok {
        debug!("reviews", "gh api rate_limit failed: {}", summarize_error(&out.stderr));
        return None;
    }
    out.stdout.trim().parse().ok()
}

/// What the review page shows for a failed search: rate limits with their
/// reset time (looked up here), anything else as gh's message.
pub fn fetch_error(gh: &dyn GhRunner, cwd: &Path, message: &str) -> ReviewFetchError {
    let rate_limited = is_rate_limit_error(message);
    let message = if is_timeout_error(message) && !rate_limited {
        format!("GitHub took too long to answer ({message}); am will try again shortly")
    } else {
        message.to_string()
    };
    ReviewFetchError {
        message,
        rate_limited,
        resets_at: if rate_limited { rate_limit_reset(gh, cwd) } else { None },
    }
}

/// The cached list, if a fetch ever succeeded.
pub fn load_cached(conn: &Connection) -> Option<Vec<ReviewRequest>> {
    let rows = load_rows(conn, CacheKind::Review).ok()?;
    let payload = rows.into_iter().find(|r| r.cache_key == CACHE_KEY)?.payload?;
    match serde_json::from_str(&payload) {
        Ok(list) => Some(list),
        Err(e) => {
            warn!("integration-cache", "Dropping unparseable review cache: {e}");
            None
        }
    }
}

/// The cached unfiltered count, if one was ever fetched.
pub fn load_cached_total(conn: &Connection) -> Option<u64> {
    let rows = load_rows(conn, CacheKind::Review).ok()?;
    rows.into_iter().find(|r| r.cache_key == TOTAL_KEY)?.payload?.parse().ok()
}

/// The cached group query hits, if a search ever returned them.
pub fn load_cached_hits(conn: &Connection) -> Option<QueryHits> {
    let rows = load_rows(conn, CacheKind::Review).ok()?;
    serde_json::from_str(&rows.into_iter().find(|r| r.cache_key == QUERIES_KEY)?.payload?).ok()
}

/// Persist the group query hits of a successful search.
pub fn store_hits(conn: &Connection, hits: &QueryHits) {
    let payload = serde_json::to_string(hits).ok();
    if let Err(e) = set_entry(conn, CacheKind::Review, QUERIES_KEY, payload.as_deref()) {
        warn!("integration-cache", "Failed to persist review group query hits: {e}");
    }
}

/// Persist a fetched unfiltered count.
pub fn store_total(conn: &Connection, total: u64) {
    if let Err(e) = set_entry(conn, CacheKind::Review, TOTAL_KEY, Some(&total.to_string())) {
        warn!("integration-cache", "Failed to persist the review total: {e}");
    }
}

/// Persist a successfully fetched list.
pub fn store(conn: &Connection, list: &[ReviewRequest]) {
    let payload = serde_json::to_string(list).ok();
    if let Err(e) = set_entry(conn, CacheKind::Review, CACHE_KEY, payload.as_deref()) {
        warn!("integration-cache", "Failed to persist review requests: {e}");
    } else {
        debug!("integration-cache", "Stored {} review request(s)", list.len());
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::integrations::github::GhOutput;
    use crate::integrations::github::tests::FakeGh;
    use serde_json::json;

    fn settings(json: &str) -> Settings {
        Settings::parse(json)
    }

    pub fn search_node(number: u64, repo: &str, head: &str, reviewers: Vec<Value>) -> Value {
        json!({
            "number": number, "title": format!("PR {number}"), "url": format!("https://github.com/{repo}/pull/{number}"),
            "state": "OPEN", "isDraft": false, "reviewDecision": "REVIEW_REQUIRED", "headRefName": head,
            "baseRefName": "main", "isCrossRepository": false, "createdAt": "2026-10-20T10:00:00Z",
            "updatedAt": "2026-10-25T10:00:00Z", "author": {"login": "alice"},
            "repository": {"nameWithOwner": repo},
            "reviewRequests": {"nodes": reviewers.into_iter().map(|r| json!({"requestedReviewer": r})).collect::<Vec<_>>()},
            "commits": {"nodes": [{"commit": {"statusCheckRollup": {"contexts": {
                "checkRunCountsByState": [{"state": "SUCCESS", "count": 2}], "statusContextCountsByState": [], "nodes": []}}}}]}
        })
    }

    #[test]
    fn search_follows_the_scope_and_repos() {
        let slugs = vec!["acme/web".to_string(), "acme/api".to_string()];
        assert_eq!(
            build_search(&settings("{}"), &slugs),
            "is:pr is:open archived:false review-requested:@me repo:acme/web repo:acme/api"
        );
        // Drafts, reviewed and repo filters are the groups' now, not the search's.
        let s = settings(r#"{"reviewRequestsScope":"direct","reviewHideDrafts":true,"reviewSearchQualifiers":"label:urgent"}"#);
        assert_eq!(build_search(&s, &slugs[..1]), "is:pr is:open archived:false user-review-requested:@me repo:acme/web");
        let teams = settings(r#"{"reviewRequestsScope":"teams"}"#);
        assert!(build_search(&teams, &[]).contains("team-review-requested-user:@me"));
    }

    #[test]
    fn search_args_are_one_line_with_the_query_as_a_variable() {
        let args = search_args("is:pr review-requested:@me", None, Extras::default());
        assert_eq!(&args[..3], ["api", "graphql", "-f"]);
        assert_eq!(args[3], "q=is:pr review-requested:@me");
        assert!(args[5].starts_with("query=query($q: String!, $after: String)"));
        assert!(!args[5].contains('\n'));
        // Check counts only: per-check nodes made a real search time out.
        assert!(args[5].contains("checkRunCountsByState") && !args[5].contains("CheckRun {"));
        assert!(args[5].contains("viewerLatestReview { state }"));
        assert_eq!(search_args("q", Some("Y3Vy"), Extras::default())[5], "after=Y3Vy");
        // The unfiltered count and the group queries ride along.
        let queries = vec!["label:urgent".to_string()];
        let args = search_args("q", None, Extras { total: true, queries: &queries });
        assert_eq!(args[5], format!("all={TOTAL_SEARCH}"));
        assert_eq!(args[7], "g0=q label:urgent");
        assert!(args[9].starts_with("query=query($q: String!, $after: String, $all: String!, $g0: String!)"));
        assert!(args[9].contains("total: search(query: $all, type: ISSUE, first: 1) { issueCount }"));
        assert!(args[9].contains("g0: search(query: $g0, type: ISSUE, first: 100) { nodes { ... on PullRequest { url } } }"));
        assert!(!search_args("q", None, Extras::default())[5].contains("issueCount"));
    }

    #[test]
    fn parses_prs_of_tracked_repos_with_their_reviewers() {
        let body = json!({"data": {"viewer": {"login": "me"}, "search": {"nodes": [
            search_node(7, "acme/web", "feature/x", vec![json!({"login": "me"}), json!({"combinedSlug": "acme/frontend"})]),
            search_node(8, "other/repo", "y", vec![]),
            {},
        ]}}})
        .to_string();
        let ids = HashMap::from([("acme/web".to_string(), "repo-1".to_string())]);
        let list = parse_search(&body, &ids, &[]).unwrap().prs;
        assert_eq!(list.len(), 1);
        let r = &list[0];
        assert_eq!((r.repo_id.as_str(), r.repo.as_str(), r.pr.number), ("repo-1", "acme/web", 7));
        assert_eq!(r.head_ref_name, "feature/x");
        assert_eq!(r.author, "alice");
        assert_eq!(r.requested, ["you", "acme/frontend"]);
        assert_eq!(r.pr.checks_status, crate::model::ChecksStatus::Passing);
        assert_eq!(r.pr.review_decision, "REVIEW_REQUIRED");
    }

    #[test]
    fn group_query_hits_and_reviewed_are_parsed() {
        let mut node = search_node(7, "acme/web", "b", vec![]);
        node["viewerLatestReview"] = json!({"state": "COMMENTED"});
        let body = json!({"data": {
            "search": {"nodes": [node, search_node(8, "acme/web", "c", vec![])]},
            "g0": {"nodes": [{"url": "https://github.com/acme/web/pull/7"}, {}]},
            "g1": {"nodes": []}
        }})
        .to_string();
        let ids = HashMap::from([("acme/web".to_string(), "r".to_string())]);
        let queries = vec!["label:a".to_string(), "label:b".to_string(), "label:c".to_string()];
        let page = parse_search(&body, &ids, &queries).unwrap();
        assert!(page.prs[0].reviewed_by_you && !page.prs[1].reviewed_by_you);
        assert_eq!(page.hits["label:a"], ["https://github.com/acme/web/pull/7"]);
        assert!(page.hits["label:b"].is_empty());
        assert!(!page.hits.contains_key("label:c"), "not answered: not known");

        let dir = tempfile::tempdir().unwrap();
        let conn = crate::db::open_at(&dir.path().join("t.db")).unwrap();
        assert!(load_cached_hits(&conn).is_none());
        store_hits(&conn, &page.hits);
        assert_eq!(load_cached_hits(&conn).unwrap(), page.hits);
    }

    #[test]
    fn approved_by_you_is_your_latest_review_approving() {
        let mut node = search_node(7, "acme/web", "b", vec![]);
        node["viewerLatestReview"] = json!({"state": "APPROVED"});
        let body = json!({"data": {"search": {"nodes": [node, search_node(8, "acme/web", "c", vec![])]}}}).to_string();
        let ids = HashMap::from([("acme/web".to_string(), "r".to_string())]);
        let prs = parse_search(&body, &ids, &[]).unwrap().prs;
        assert!(prs[0].approved_by_you && prs[0].reviewed_by_you && !prs[0].approved_only);
        assert!(!prs[1].approved_by_you);
    }

    #[test]
    fn pulled_args_look_up_each_branch_on_one_line() {
        let pulled = vec![
            Pulled { repo_id: "r1".into(), slug: "acme/web".into(), branch: "feat/a".into() },
            Pulled { repo_id: "r2".into(), slug: "acme/api".into(), branch: "fix/b".into() },
        ];
        let args = pulled_args(&pulled);
        assert_eq!(&args[..2], ["api", "graphql"]);
        for v in ["o0=acme", "n0=web", "b0=feat/a", "o1=acme", "n1=api", "b1=fix/b"] {
            assert!(args.contains(&v.to_string()), "{v} in {args:?}");
        }
        let query = args.last().unwrap();
        assert!(query.starts_with("query=query($o0: String!, $n0: String!, $b0: String!, $o1: String!"));
        assert!(query.contains("viewer { login }"));
        assert!(query.contains("p1: repository(owner: $o1, name: $n1) { pullRequests(headRefName: $b1, first: 1,"));
        assert!(query.contains("viewerLatestReview { state }"));
        assert!(!query.contains('\n'));
    }

    #[test]
    fn parse_pulled_keeps_only_prs_you_approved() {
        let pulled: Vec<Pulled> = ["a", "b", "c", "d"]
            .iter()
            .map(|b| Pulled { repo_id: "r1".into(), slug: "acme/web".into(), branch: b.to_string() })
            .collect();
        let with = |head: &str, state: Option<&str>| {
            let mut node = search_node(1, "acme/web", head, vec![json!({"login": "me"})]);
            if let Some(s) = state {
                node["viewerLatestReview"] = json!({"state": s});
            }
            node["state"] = json!("MERGED");
            json!({"pullRequests": {"nodes": [node]}})
        };
        let body = json!({"data": {
            "viewer": {"login": "me"},
            "p0": with("a", Some("APPROVED")),
            "p1": with("b", Some("COMMENTED")),
            "p2": {"pullRequests": {"nodes": []}},
            "p3": with("d", None),
        }})
        .to_string();
        let list = parse_pulled(&body, &pulled).unwrap();
        assert_eq!(list.len(), 1);
        let r = &list[0];
        assert!(r.approved_only && r.approved_by_you && r.head_ref_name == "a");
        assert_eq!((r.repo_id.as_str(), r.repo.as_str(), r.pr.state.as_str()), ("r1", "acme/web", "MERGED"));
        assert_eq!(r.requested, ["you"]);
        assert!(parse_pulled(r#"{"errors":[{"message":"nope"}]}"#, &pulled).unwrap_err().contains("nope"));
    }

    #[test]
    fn errors_are_classified() {
        let ids = HashMap::new();
        let gh = FakeGh::new(vec![
            GhOutput { ok: false, stdout: String::new(), stderr: "error connecting to api.github.com".into() },
            GhOutput { ok: false, stdout: r#"{"errors":[{"message":"bad query"}]}"#.into(), stderr: String::new() },
        ]);
        let (_, network) = fetch(&gh, Path::new("."), "q", &ids, Extras::default()).unwrap_err();
        assert!(network);
        let (msg, network) = fetch(&gh, Path::new("."), "q", &ids, Extras::default()).unwrap_err();
        assert!(!network && msg.contains("bad query"), "{msg}");
    }

    #[test]
    fn pages_follow_the_cursor_up_to_the_limit() {
        let ids = HashMap::from([("acme/web".to_string(), "r".to_string())]);
        let page = |n: u64, next: Option<&str>| {
            json!({"data": {"search": {
                "pageInfo": {"hasNextPage": next.is_some(), "endCursor": next},
                "nodes": [search_node(n, "acme/web", "b", vec![])]}}})
            .to_string()
        };
        let gh = FakeGh::new(vec![GhOutput::success(page(1, Some("c1"))), GhOutput::success(page(2, Some("c2")))]);
        let list = fetch(&gh, Path::new("."), "q", &ids, Extras::default()).unwrap().prs;
        assert_eq!(list.iter().map(|r| r.pr.number).collect::<Vec<_>>(), [1, 2]);
        let calls = gh.calls.lock().unwrap();
        assert_eq!(calls.len(), MAX_PAGES, "stops at the page limit");
        assert!(calls[1].contains(&"after=c1".to_string()));
    }

    #[test]
    fn the_unfiltered_total_comes_with_the_first_page_only() {
        let ids = HashMap::from([("acme/web".to_string(), "r".to_string())]);
        let page = |next: Option<&str>, total: Option<u64>| {
            let mut body = json!({"data": {"search": {
                "pageInfo": {"hasNextPage": next.is_some(), "endCursor": next},
                "nodes": [search_node(1, "acme/web", "b", vec![])]}}});
            if let Some(n) = total {
                body["data"]["total"] = json!({"issueCount": n});
            }
            body.to_string()
        };
        let gh = FakeGh::new(vec![GhOutput::success(page(Some("c1"), Some(42))), GhOutput::success(page(None, None))]);
        let got = fetch(&gh, Path::new("."), "q", &ids, Extras { total: true, queries: &[] }).unwrap();
        assert_eq!((got.prs.len(), got.total), (2, Some(42)));
        let calls = gh.calls.lock().unwrap();
        assert!(calls[0].iter().any(|a| a.starts_with("all=")));
        assert!(!calls[1].iter().any(|a| a.starts_with("all=")));

        let dir = tempfile::tempdir().unwrap();
        let conn = crate::db::open_at(&dir.path().join("t.db")).unwrap();
        assert_eq!(load_cached_total(&conn), None);
        store_total(&conn, 42);
        assert_eq!(load_cached_total(&conn), Some(42));
        assert!(load_cached(&conn).is_none(), "the total doesn't pass for a list");
    }

    #[test]
    fn a_github_timeout_is_reported_as_one_not_as_bad_output() {
        let ids = HashMap::new();
        let html = "<!DOCTYPE html>\n<!--\n\nHello future GitHubber!";
        let gh = FakeGh::new(vec![GhOutput { ok: false, stdout: html.into(), stderr: "gh: HTTP 504\n".into() }]);
        let (msg, network) = fetch(&gh, Path::new("."), "q", &ids, Extras::default()).unwrap_err();
        assert_eq!(msg, "HTTP 504");
        assert!(network, "backs off");
        let e = fetch_error(&gh, Path::new("."), &msg);
        assert!(!e.rate_limited);
        assert!(e.message.starts_with("GitHub took too long to answer (HTTP 504)"), "{}", e.message);
    }

    #[test]
    fn a_rate_limit_is_reported_with_its_reset_time() {
        let ids = HashMap::new();
        let body = r#"{"errors":[{"type":"RATE_LIMITED","message":"API rate limit exceeded for user ID 1."}]}"#;
        let gh = FakeGh::new(vec![
            GhOutput { ok: false, stdout: body.into(), stderr: String::new() },
            GhOutput::success("1793016000\n"),
        ]);
        let (msg, network) = fetch(&gh, Path::new("."), "q", &ids, Extras::default()).unwrap_err();
        assert!(network, "a rate limit backs off");
        let e = fetch_error(&gh, Path::new("."), &msg);
        assert!(e.rate_limited);
        assert_eq!(e.resets_at, Some(1_793_016_000));
        assert_eq!(gh.calls.lock().unwrap()[1], ["api", "rate_limit", "--jq", ".resources.graphql.reset"]);
        // Anything else: just the message, no extra call.
        let other = fetch_error(&gh, Path::new("."), "HTTP 401: Bad credentials");
        assert!(!other.rate_limited && other.resets_at.is_none());
        assert_eq!(gh.call_count(), 2);
    }

    #[test]
    fn slug_comes_from_gh_repo_view() {
        let gh = FakeGh::new(vec![GhOutput::success("acme/web\n"), GhOutput::failure("no git remotes found")]);
        assert_eq!(repo_slug(&gh, Path::new(".")).unwrap(), "acme/web");
        assert_eq!(gh.calls.lock().unwrap()[0], ["repo", "view", "--json", "nameWithOwner", "-q", ".nameWithOwner"]);
        assert!(!repo_slug(&gh, Path::new(".")).unwrap_err().1);
    }

    #[test]
    fn people_query_is_one_line_with_variables() {
        let args = people_args(&["acme/web".to_string(), "acme/api".to_string()]);
        assert_eq!(&args[..2], ["api", "graphql"]);
        assert!(args.contains(&"o0=acme".to_string()) && args.contains(&"n1=api".to_string()));
        let query = args.last().unwrap();
        assert!(query.starts_with("query=query($o0: String!, $n0: String!, $o1: String!, $n1: String!)"), "{query}");
        assert!(query.contains("r1: repository(owner: $o1, name: $n1) { assignableUsers(first: 100)"));
        assert!(!query.contains('\n'));
    }

    #[test]
    fn people_are_merged_across_repos() {
        let body = json!({"data": {
            "r0": {"assignableUsers": {"nodes": [{"login": "bob", "name": null}, {"login": "alice", "name": "Alice L"}]}},
            "r1": {"assignableUsers": {"nodes": [{"login": "Alice", "name": "Alice L"}, {"login": "carol", "name": "Carol"}]}},
            "r2": null
        }})
        .to_string();
        let people = parse_people(&body).unwrap();
        assert_eq!(people.iter().map(|p| p.login.as_str()).collect::<Vec<_>>(), ["alice", "bob", "carol"]);
        assert_eq!(people[0].name, "Alice L");
        assert_eq!(people[1].name, "");
        assert!(parse_people(r#"{"errors":[{"message":"nope"}]}"#).unwrap_err().contains("nope"));

        let dir = tempfile::tempdir().unwrap();
        let conn = crate::db::open_at(&dir.path().join("t.db")).unwrap();
        assert!(load_cached_people(&conn).is_none());
        store_people(&conn, &people);
        assert_eq!(load_cached_people(&conn).unwrap(), people);
    }

    #[test]
    fn cache_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let conn = crate::db::open_at(&dir.path().join("t.db")).unwrap();
        assert!(load_cached(&conn).is_none());
        let body = json!({"data": {"search": {"nodes": [search_node(7, "acme/web", "feature/x", vec![])]}}}).to_string();
        let ids = HashMap::from([("acme/web".to_string(), "repo-1".to_string())]);
        let list = parse_search(&body, &ids, &[]).unwrap().prs;
        store(&conn, &list);
        assert_eq!(load_cached(&conn).unwrap(), list);
    }
}
