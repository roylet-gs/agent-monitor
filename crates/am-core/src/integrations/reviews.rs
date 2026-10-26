//! Open PRs waiting on your review, in the tracked repos.
//!
//! One `gh api graphql` `search(type: ISSUE)` call per refresh, built from the
//! `review*` settings ([`build_search`]): `review-requested:@me` (you or a
//! team you're on; GitHub drops a PR from it once you've reviewed), narrowed
//! by scope, drafts, `-reviewed-by:@me`, one `repo:` per selected tracked repo
//! and any extra qualifiers. Each tracked repo's `owner/name` comes from
//! `gh repo view` in its checkout, so gh's own remote rules apply (and the
//! E2E shim can answer for a local bare origin).
//!
//! The list is cached as a single `integration_cache` row (`kind = "review"`,
//! `cache_key = "requests"`) and, like PRs and tickets, only a successful
//! fetch replaces it, so it survives going offline.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::Connection;
use serde_json::Value;

use super::breaker::{is_network_error, is_rate_limit_error, is_timeout_error, summarize_error};
use super::cache::{CacheKind, load_rows, set_entry};
use super::github::{GhRunner, pr_info_from_node, pr_selection_counts};
use crate::model::{ReviewFetchError, ReviewRequest};
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

/// A tracked repo the search may cover.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewRepo {
    pub repo_id: String,
    /// The repository's display name (its directory name).
    pub name: String,
    pub path: PathBuf,
}

impl ReviewRepo {
    pub fn new(repo_id: impl Into<String>, name: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        ReviewRepo { repo_id: repo_id.into(), name: name.into(), path: path.into() }
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

/// Comma/space separated repo list from the settings.
fn repo_list(raw: &str) -> Vec<String> {
    raw.split([',', ' ', '\t', '\n']).map(str::trim).filter(|s| !s.is_empty()).map(str::to_lowercase).collect()
}

/// A list entry names this repo by directory name, `owner/name`, or GitHub name.
fn listed(list: &[String], name: &str, slug: &str) -> bool {
    let (name, slug) = (name.to_lowercase(), slug.to_lowercase());
    let short = slug.rsplit('/').next().unwrap_or("");
    list.iter().any(|e| *e == name || *e == slug || *e == short)
}

/// Whether the include/exclude settings let `(name, slug)` in.
pub fn repo_selected(settings: &Settings, name: &str, slug: &str) -> bool {
    let include = repo_list(&settings.review_include_repos);
    let exclude = repo_list(&settings.review_exclude_repos);
    (include.is_empty() || listed(&include, name, slug)) && !listed(&exclude, name, slug)
}

/// The GitHub search string for `slugs` (already filtered).
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
    if settings.review_hide_drafts {
        q.push("draft:false".into());
    }
    if settings.review_hide_reviewed {
        q.push("-reviewed-by:@me".into());
    }
    q.extend(slugs.iter().map(|s| format!("repo:{s}")));
    let extra = settings.review_search_qualifiers.split_whitespace().collect::<Vec<_>>().join(" ");
    if !extra.is_empty() {
        q.push(extra);
    }
    q.join(" ")
}

/// `gh api graphql` argv for one page of a search. The search string (and
/// the page cursor) travel as variables; the query text is a single line (the
/// E2E shim splits argv on newlines). Checks are counts only
/// ([`pr_selection_counts`]), which keeps a page well inside GitHub's 10 s.
pub fn search_args(search: &str, after: Option<&str>) -> Vec<String> {
    let query = format!(
        "query($q: String!, $after: String) {{ viewer {{ login }} \
         search(query: $q, type: ISSUE, first: {PAGE_SIZE}, after: $after) {{ pageInfo {{ hasNextPage endCursor }} nodes {{ \
         ... on PullRequest {{ {} createdAt updatedAt author {{ login }} repository {{ nameWithOwner }} \
         reviewRequests(first: 20) {{ nodes {{ requestedReviewer {{ ... on User {{ login }} \
         ... on Team {{ combinedSlug }} ... on Bot {{ login }} ... on Mannequin {{ login }} }} }} }} }} }} }} }}",
        pr_selection_counts()
    );
    let mut args: Vec<String> = ["api", "graphql", "-f"].iter().map(|s| s.to_string()).collect();
    args.push(format!("q={search}"));
    if let Some(cursor) = after {
        args.extend(["-f".into(), format!("after={cursor}")]);
    }
    args.extend(["-f".into(), format!("query={query}")]);
    args
}

/// One parsed page: its PRs and the cursor of the next page, if any.
#[derive(Debug)]
pub struct SearchPage {
    pub prs: Vec<ReviewRequest>,
    pub next: Option<String>,
}

/// Parse one page of a search response. `repo_ids` maps lower-cased
/// `owner/name` to the tracked repo id; PRs in any other repo are dropped. A
/// pending reviewer who is the viewer reads `you`.
pub fn parse_search(stdout: &str, repo_ids: &HashMap<String, String>) -> Result<SearchPage, String> {
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
        let Some(pr) = pr_info_from_node(node) else { continue };
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
        out.push(ReviewRequest {
            repo_id: repo_id.clone(),
            repo,
            pr,
            author: str_at(node, "/author/login"),
            head_ref_name: str_at(node, "/headRefName"),
            is_cross_repository: node.get("isCrossRepository").and_then(Value::as_bool).unwrap_or(false),
            created_at: str_at(node, "/createdAt"),
            updated_at: str_at(node, "/updatedAt"),
            requested,
            linear: None,
            worktree_id: None,
        });
    }
    let next = (json.pointer("/data/search/pageInfo/hasNextPage").and_then(Value::as_bool) == Some(true))
        .then(|| str_at(&json, "/data/search/pageInfo/endCursor"))
        .filter(|c| !c.is_empty());
    Ok(SearchPage { prs: out, next })
}

/// Run a search, page by page (at most [`MAX_PAGES`]). `Err((message,
/// network))`; a failed page fails the whole search, so the cached list
/// stays rather than being replaced by part of it.
pub fn fetch(
    gh: &dyn GhRunner,
    cwd: &Path,
    search: &str,
    repo_ids: &HashMap<String, String>,
) -> Result<Vec<ReviewRequest>, (String, bool)> {
    let mut all = Vec::new();
    let mut after: Option<String> = None;
    for page in 0..MAX_PAGES {
        let out = gh.run(cwd, &search_args(search, after.as_deref()), SEARCH_TIMEOUT);
        // gh exits 1 on GraphQL errors but still prints the JSON body; on an
        // HTTP error (a 504 when GitHub's 10 s ran out) stdout is GitHub's
        // HTML error page and stderr says what happened.
        if !out.stdout.trim_start().starts_with('{') {
            let err = if out.ok { "unexpected gh graphql output".to_string() } else { out.stderr.clone() };
            return Err((summarize_error(&err), !out.ok && is_network_error(&err)));
        }
        // A rate limit comes back as a GraphQL error in the body.
        let parsed = parse_search(&out.stdout, repo_ids).map_err(|e| (summarize_error(&e), is_network_error(&e)))?;
        all.extend(parsed.prs);
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
    fn search_follows_the_settings() {
        let slugs = vec!["acme/web".to_string(), "acme/api".to_string()];
        assert_eq!(
            build_search(&settings("{}"), &slugs),
            "is:pr is:open archived:false review-requested:@me draft:false -reviewed-by:@me repo:acme/web repo:acme/api"
        );
        let s = settings(
            r#"{"reviewRequestsScope":"direct","reviewHideDrafts":false,"reviewHideReviewed":false,
                "reviewSearchQualifiers":"  label:urgent   -author:app/dependabot "}"#,
        );
        assert_eq!(
            build_search(&s, &slugs[..1]),
            "is:pr is:open archived:false user-review-requested:@me repo:acme/web label:urgent -author:app/dependabot"
        );
        let teams = settings(r#"{"reviewRequestsScope":"teams"}"#);
        assert!(build_search(&teams, &[]).contains("team-review-requested-user:@me"));
    }

    #[test]
    fn include_and_exclude_match_name_or_slug() {
        let all = settings("{}");
        assert!(repo_selected(&all, "web", "acme/web"));
        let only = settings(r#"{"reviewIncludeRepos":"acme/web, api"}"#);
        assert!(repo_selected(&only, "web", "acme/web"));
        assert!(repo_selected(&only, "api-checkout", "acme/API"));
        assert!(!repo_selected(&only, "docs", "acme/docs"));
        let not = settings(r#"{"reviewExcludeRepos":"docs"}"#);
        assert!(!repo_selected(&not, "docs", "acme/docs"));
        assert!(repo_selected(&not, "web", "acme/web"));
    }

    #[test]
    fn search_args_are_one_line_with_the_query_as_a_variable() {
        let args = search_args("is:pr review-requested:@me", None);
        assert_eq!(&args[..3], ["api", "graphql", "-f"]);
        assert_eq!(args[3], "q=is:pr review-requested:@me");
        assert!(args[5].starts_with("query=query($q: String!, $after: String)"));
        assert!(!args[5].contains('\n'));
        // Check counts only: per-check nodes made a real search time out.
        assert!(args[5].contains("checkRunCountsByState") && !args[5].contains("CheckRun {"));
        assert_eq!(search_args("q", Some("Y3Vy"))[5], "after=Y3Vy");
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
        let list = parse_search(&body, &ids).unwrap().prs;
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
    fn errors_are_classified() {
        let ids = HashMap::new();
        let gh = FakeGh::new(vec![
            GhOutput { ok: false, stdout: String::new(), stderr: "error connecting to api.github.com".into() },
            GhOutput { ok: false, stdout: r#"{"errors":[{"message":"bad query"}]}"#.into(), stderr: String::new() },
        ]);
        let (_, network) = fetch(&gh, Path::new("."), "q", &ids).unwrap_err();
        assert!(network);
        let (msg, network) = fetch(&gh, Path::new("."), "q", &ids).unwrap_err();
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
        let list = fetch(&gh, Path::new("."), "q", &ids).unwrap();
        assert_eq!(list.iter().map(|r| r.pr.number).collect::<Vec<_>>(), [1, 2]);
        let calls = gh.calls.lock().unwrap();
        assert_eq!(calls.len(), MAX_PAGES, "stops at the page limit");
        assert!(calls[1].contains(&"after=c1".to_string()));
    }

    #[test]
    fn a_github_timeout_is_reported_as_one_not_as_bad_output() {
        let ids = HashMap::new();
        let html = "<!DOCTYPE html>\n<!--\n\nHello future GitHubber!";
        let gh = FakeGh::new(vec![GhOutput { ok: false, stdout: html.into(), stderr: "gh: HTTP 504\n".into() }]);
        let (msg, network) = fetch(&gh, Path::new("."), "q", &ids).unwrap_err();
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
        let (msg, network) = fetch(&gh, Path::new("."), "q", &ids).unwrap_err();
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
    fn cache_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let conn = crate::db::open_at(&dir.path().join("t.db")).unwrap();
        assert!(load_cached(&conn).is_none());
        let body = json!({"data": {"search": {"nodes": [search_node(7, "acme/web", "feature/x", vec![])]}}}).to_string();
        let ids = HashMap::from([("acme/web".to_string(), "repo-1".to_string())]);
        let list = parse_search(&body, &ids).unwrap().prs;
        store(&conn, &list);
        assert_eq!(load_cached(&conn).unwrap(), list);
    }
}
