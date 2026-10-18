//! GitHub PR status via the `gh` CLI (port of `src/lib/github.ts`).
//!
//! The TS app ran one `gh pr view <branch>` per branch. Here every branch of a
//! repo is fetched with **one** `gh api graphql` call (chunked at
//! [`GRAPHQL_CHUNK`] branches), one aliased `pullRequests(headRefName:)`
//! connection per branch. `{owner}`/`{repo}` are filled in by gh from the
//! repo's remote, so this works wherever `gh pr view` did.
//!
//! If the GraphQL call fails with a *non-network* error (a gh too old for
//! `api graphql`, a GraphQL error, unparseable output, …) the batch falls back
//! to the TS behaviour of one `gh pr view` per branch. A network-class failure
//! never falls back — it aborts the batch so the caller's breaker can back off.
//!
//! We keep shelling out to `gh` (rather than calling the API directly) so the
//! user's gh auth is reused and the Docker E2E `gh` shim keeps working.

use std::collections::HashSet;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::Value;

use super::FetchResult;
use super::breaker::{is_network_error, summarize_error};
use crate::model::{ChecksStatus, PrInfo};
use crate::{debug, info};

/// Max branches per GraphQL request (5 PRs × 100 checks each stays far below
/// GitHub's node limit).
pub const GRAPHQL_CHUNK: usize = 40;
pub const GRAPHQL_TIMEOUT: Duration = Duration::from_secs(15);
pub const PR_VIEW_TIMEOUT: Duration = Duration::from_secs(5);

const GH_PR_FIELDS: &str = "number,title,url,state,isDraft,reviewDecision,mergeable,headRefName,baseRefName,statusCheckRollup";

// ---------------------------------------------------------------------------
// gh runner (injectable)
// ---------------------------------------------------------------------------

/// Result of one `gh` invocation. Spawn failures and timeouts are reported as
/// `ok: false` with a descriptive `stderr` ("gh timed out after 15s" is
/// classified as a network error by [`is_network_error`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GhOutput {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

impl GhOutput {
    pub fn success(stdout: impl Into<String>) -> Self {
        GhOutput { ok: true, stdout: stdout.into(), stderr: String::new() }
    }
    pub fn failure(stderr: impl Into<String>) -> Self {
        GhOutput { ok: false, stdout: String::new(), stderr: stderr.into() }
    }
}

/// Runs `gh <args>` in `cwd`. Swapped for a fixture runner in tests.
pub trait GhRunner: Send {
    fn run(&self, cwd: &Path, args: &[String], timeout: Duration) -> GhOutput;
}

/// The real `gh` on `$PATH` (the E2E shim in Docker).
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemGh;

impl GhRunner for SystemGh {
    fn run(&self, cwd: &Path, args: &[String], timeout: Duration) -> GhOutput {
        run_program("gh", cwd, args, timeout, &[("GH_PROMPT_DISABLED", "1"), ("NO_COLOR", "1")])
    }
}

/// Run `program <args>` in `cwd` with stdin closed, killing it after
/// `timeout`. Shared by [`SystemGh`] and `gitpr::SystemGit`.
pub(crate) fn run_program(program: &str, cwd: &Path, args: &[String], timeout: Duration, envs: &[(&str, &str)]) -> GhOutput {
    let spawned = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .envs(envs.iter().copied())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match spawned {
        Ok(c) => c,
        Err(e) => return GhOutput::failure(format!("failed to run {program}: {e}")),
    };
    // Drain both pipes on threads so a chatty child can't deadlock on a full pipe.
    let mut out = child.stdout.take();
    let mut err = child.stderr.take();
    let t_out = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(o) = out.as_mut() {
            let _ = o.read_to_string(&mut s);
        }
        s
    });
    let t_err = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(e) = err.as_mut() {
            let _ = e.read_to_string(&mut s);
        }
        s
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => break None,
        }
    };
    let stdout = t_out.join().unwrap_or_default();
    let stderr = t_err.join().unwrap_or_default();
    match status {
        Some(s) => GhOutput { ok: s.success(), stdout, stderr },
        None => GhOutput { ok: false, stdout, stderr: format!("{program} timed out after {}s", timeout.as_secs()) },
    }
}

// ---------------------------------------------------------------------------
// Checks / PR derivation (pure)
// ---------------------------------------------------------------------------

/// One CI check, normalised from either a CheckRun (`status`/`conclusion`) or
/// a commit StatusContext (`state`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Check {
    /// QUEUED | IN_PROGRESS | COMPLETED | WAITING | PENDING | REQUESTED
    pub status: String,
    /// SUCCESS | FAILURE | ERROR | CANCELLED | ACTION_REQUIRED | NEUTRAL | SKIPPED | … or ""
    pub conclusion: String,
    pub name: Option<String>,
    pub details_url: Option<String>,
}

/// Raw check as found in both `gh pr view --json statusCheckRollup` and the
/// GraphQL `statusCheckRollup.contexts` union.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RawCheck {
    status: Option<String>,
    conclusion: Option<String>,
    /// StatusContext only.
    state: Option<String>,
    name: Option<String>,
    /// StatusContext's name.
    context: Option<String>,
    details_url: Option<String>,
    target_url: Option<String>,
}

impl RawCheck {
    fn normalize(self) -> Check {
        if self.status.is_none()
            && self.conclusion.is_none()
            && let Some(state) = self.state
        {
            // StatusContext. The TS code read `status` here, which is absent, so
            // every commit status counted as pending forever.
            let terminal = matches!(state.as_str(), "SUCCESS" | "FAILURE" | "ERROR");
            return Check {
                status: if terminal { "COMPLETED".into() } else { "PENDING".into() },
                conclusion: if terminal { state } else { String::new() },
                name: self.context.or(self.name),
                details_url: self.target_url.or(self.details_url),
            };
        }
        Check {
            status: self.status.unwrap_or_default(),
            conclusion: self.conclusion.unwrap_or_default(),
            name: self.name.or(self.context),
            details_url: self.details_url.or(self.target_url),
        }
    }
}

/// FAILURE / ERROR / CANCELLED as in the TS app, plus TIMED_OUT and
/// STARTUP_FAILURE (which the TS app counted as passing).
fn is_failed(c: &Check) -> bool {
    matches!(c.conclusion.as_str(), "FAILURE" | "ERROR" | "CANCELLED" | "TIMED_OUT" | "STARTUP_FAILURE")
}

fn is_pending(c: &Check) -> bool {
    c.status != "COMPLETED"
}

/// Port of `deriveChecksStatus`: any failure → failing, else any incomplete →
/// pending, else passing; no checks → none.
pub fn derive_checks_status(checks: &[Check]) -> ChecksStatus {
    if checks.is_empty() {
        ChecksStatus::None
    } else if checks.iter().any(is_failed) {
        ChecksStatus::Failing
    } else if checks.iter().any(is_pending) {
        ChecksStatus::Pending
    } else {
        ChecksStatus::Passing
    }
}

/// The PR fields we need, independent of where they came from.
#[derive(Debug, Clone, Default)]
struct PrFields {
    number: u64,
    title: String,
    url: String,
    state: String,
    is_draft: bool,
    review_decision: Option<String>,
    mergeable: Option<String>,
    base_ref_name: Option<String>,
    /// Possibly only the first page of checks (see `summary`).
    checks: Vec<Check>,
    /// Status computed from GitHub's per-state counts over *all* checks; when
    /// present it wins over deriving from `checks`, which may be truncated.
    summary: Option<CheckSummary>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CheckSummary {
    status: ChecksStatus,
    waiting: bool,
}

/// `checkRunCountsByState` / `statusContextCountsByState` → the same status
/// rules as [`derive_checks_status`], without having to page through every
/// check (a monorepo PR had 357, so the first page missed all 6 failures).
fn summarize_counts(rollup: &Value) -> Option<CheckSummary> {
    let contexts = rollup.get("contexts")?;
    let mut total = 0u64;
    let (mut failed, mut pending, mut waiting) = (false, false, false);
    for (field, failing, incomplete) in [
        (
            "checkRunCountsByState",
            &["FAILURE", "CANCELLED", "TIMED_OUT", "STARTUP_FAILURE"][..],
            &["QUEUED", "IN_PROGRESS", "PENDING", "WAITING", "REQUESTED"][..],
        ),
        ("statusContextCountsByState", &["FAILURE", "ERROR"][..], &["PENDING", "EXPECTED"][..]),
    ] {
        for entry in contexts.get(field)?.as_array()? {
            let state = entry.get("state").and_then(Value::as_str).unwrap_or("");
            let count = entry.get("count").and_then(Value::as_u64).unwrap_or(0);
            if count == 0 {
                continue;
            }
            total += count;
            failed |= failing.contains(&state);
            pending |= incomplete.contains(&state);
            waiting |= field == "checkRunCountsByState" && matches!(state, "WAITING" | "ACTION_REQUIRED");
        }
    }
    let status = if total == 0 {
        ChecksStatus::None
    } else if failed {
        ChecksStatus::Failing
    } else if pending {
        ChecksStatus::Pending
    } else {
        ChecksStatus::Passing
    };
    Some(CheckSummary { status, waiting })
}

/// Most relevant active check: failing first, then pending/waiting.
fn active_check(checks: &[Check]) -> Option<&Check> {
    checks.iter().find(|c| is_failed(c)).or_else(|| checks.iter().find(|c| is_pending(c)))
}

/// Port of `ghResultToPrInfo`.
fn to_pr_info(pr: PrFields) -> PrInfo {
    let checks_status = pr.summary.map_or_else(|| derive_checks_status(&pr.checks), |s| s.status);
    let active = active_check(&pr.checks);
    let checks_waiting = pr.summary.map_or_else(
        || pr.checks.iter().any(|c| c.status == "WAITING" || c.conclusion == "ACTION_REQUIRED"),
        |s| s.waiting,
    );
    PrInfo {
        number: pr.number,
        title: pr.title,
        url: pr.url,
        state: pr.state,
        is_draft: pr.is_draft,
        review_decision: pr.review_decision.unwrap_or_default(),
        // Not derived by the TS app either; kept for payload compatibility.
        has_feedback: false,
        checks_status,
        active_check_url: active.and_then(|c| c.details_url.clone()),
        active_check_name: active.and_then(|c| c.name.clone()),
        checks_waiting,
        base_ref_name: pr.base_ref_name,
        mergeable: pr.mergeable,
    }
}

/// Port of `getPrStatusLabel`: (label, colour name).
pub fn pr_status_label(pr: &PrInfo) -> (&'static str, &'static str) {
    let checks = pr.checks_status;
    match pr.state.as_str() {
        "MERGED" => {
            return if checks == ChecksStatus::Failing {
                ("Merged - Actions Failing", "red")
            } else if pr.checks_waiting {
                ("Merged - Awaiting Approval", "yellow")
            } else if checks == ChecksStatus::Pending {
                ("Merged - Actions Running", "magenta")
            } else {
                ("Merged", "magenta")
            };
        }
        "CLOSED" => return ("Closed", "red"),
        _ => {}
    }
    match pr.review_decision.as_str() {
        "CHANGES_REQUESTED" => return ("Changes Requested", "red"),
        "APPROVED" => {
            return match checks {
                ChecksStatus::Failing => ("Approved - Checks Failing", "yellow"),
                ChecksStatus::Pending => ("Approved - Checks Running", "cyan"),
                _ if pr.mergeable.as_deref() == Some("CONFLICTING") => ("Approved - Conflicts", "yellow"),
                _ => ("Approved", "green"),
            };
        }
        _ => {}
    }
    if pr.is_draft {
        return if pr.has_feedback { ("Draft - Feedback", "yellow") } else { ("Draft", "gray") };
    }
    if pr.has_feedback {
        return ("In Review - Feedback", "yellow");
    }
    match checks {
        ChecksStatus::Failing => ("In Review - Checks Failing", "yellow"),
        ChecksStatus::Pending => ("In Review - Checks Running", "cyan"),
        _ => ("In Review", "cyan"),
    }
}

// ---------------------------------------------------------------------------
// Batched GraphQL fetch
// ---------------------------------------------------------------------------

/// Outcome of fetching a set of branches for one repo.
#[derive(Debug)]
pub struct PrBatch {
    /// One entry per requested branch, in request order.
    pub results: Vec<(String, FetchResult<PrInfo>)>,
    /// Set when a network-class failure aborted the batch. Branches not
    /// fetched by then carry `FetchResult::Err` (so their cache survives).
    pub network_error: Option<String>,
    /// How many `gh` processes were spawned (for logs/tests).
    pub gh_calls: usize,
    /// GraphQL points left this hour, from the last response's `rateLimit`.
    pub rate_remaining: Option<u64>,
}

/// Selection set for one page of rollup contexts (CheckRun | StatusContext).
const CHECK_NODES: &str = "pageInfo { hasNextPage endCursor } nodes { __typename \
     ... on CheckRun { name status conclusion detailsUrl } \
     ... on StatusContext { context state targetUrl } }";

/// Extra pages fetched (per PR, only when needed) to find the active check
/// when the first 100 don't contain it.
pub const EXTRA_CHECK_PAGES: usize = 4;

fn build_query(n: usize) -> String {
    let mut vars = String::from("$owner: String!, $name: String!");
    let mut body = String::new();
    for i in 0..n {
        vars.push_str(&format!(", $b{i}: String!"));
        body.push_str(&format!(
            " b{i}: pullRequests(headRefName: $b{i}, first: 5, states: [OPEN, MERGED, CLOSED], \
             orderBy: {{field: CREATED_AT, direction: DESC}}) {{ ...pr }}"
        ));
    }
    // Single line on purpose: the E2E gh shim splits argv on newlines.
    format!(
        "query({vars}) {{ rateLimit {{ cost remaining }} repository(owner: $owner, name: $name) {{{body} }} }} \
         fragment pr on PullRequestConnection {{ nodes {{ number title url state isDraft reviewDecision mergeable \
         headRefName baseRefName isCrossRepository commits(last: 1) {{ nodes {{ commit {{ statusCheckRollup {{ \
         contexts(first: 100) {{ checkRunCountsByState {{ state count }} \
         statusContextCountsByState {{ state count }} {CHECK_NODES} }} }} }} }} }} }} }}"
    )
}

/// `gh api graphql` argv for one chunk. Branch names travel as typed
/// variables (`-f bN=<branch>`), never interpolated into the query text.
pub fn graphql_args(branches: &[String]) -> Vec<String> {
    let mut args = repo_args();
    for (i, b) in branches.iter().enumerate() {
        args.push("-f".into());
        args.push(format!("b{i}={b}"));
    }
    args.push("-f".into());
    args.push(format!("query={}", build_query(branches.len())));
    args
}

/// `gh api graphql` with gh filling `{owner}`/`{repo}` from the checkout's remote.
fn repo_args() -> Vec<String> {
    ["api", "graphql", "-F", "owner={owner}", "-F", "name={repo}"].iter().map(|s| s.to_string()).collect()
}

fn more_checks_args(number: u64, cursor: &str) -> Vec<String> {
    let mut args = repo_args();
    args.extend(["-F".into(), format!("n={number}"), "-f".into(), format!("after={cursor}"), "-f".into()]);
    args.push(format!(
        "query=query($owner: String!, $name: String!, $n: Int!, $after: String) {{ \
         repository(owner: $owner, name: $name) {{ pullRequest(number: $n) {{ commits(last: 1) {{ nodes {{ \
         commit {{ statusCheckRollup {{ contexts(first: 100, after: $after) {{ {CHECK_NODES} }} }} }} }} }} }} }} }}"
    ));
    args
}

/// Checks on one page of `contexts`, plus the cursor of the next page if any.
fn parse_contexts(contexts: Option<&Value>) -> (Vec<Check>, Option<String>) {
    let Some(contexts) = contexts else { return (Vec::new(), None) };
    let checks = contexts
        .get("nodes")
        .and_then(Value::as_array)
        .map(|ns| {
            ns.iter().filter_map(|n| serde_json::from_value::<RawCheck>(n.clone()).ok()).map(RawCheck::normalize).collect()
        })
        .unwrap_or_default();
    let next = if contexts.pointer("/pageInfo/hasNextPage").and_then(Value::as_bool) == Some(true) {
        contexts.pointer("/pageInfo/endCursor").and_then(Value::as_str).map(str::to_string)
    } else {
        None
    };
    (checks, next)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GqlPr {
    number: u64,
    #[serde(default)]
    title: String,
    #[serde(default)]
    url: String,
    state: String,
    #[serde(default)]
    is_draft: bool,
    #[serde(default)]
    review_decision: Option<String>,
    #[serde(default)]
    mergeable: Option<String>,
    #[serde(default)]
    head_ref_name: Option<String>,
    #[serde(default)]
    base_ref_name: Option<String>,
    #[serde(default)]
    is_cross_repository: bool,
    #[serde(default)]
    commits: Option<Value>,
}

/// A parsed PR plus the cursor for more checks, if GitHub has more.
#[derive(Debug)]
struct ParsedPr {
    fields: PrFields,
    next_checks: Option<String>,
}

impl GqlPr {
    fn into_parsed(self) -> ParsedPr {
        let rollup = self.commits.as_ref().and_then(|c| c.pointer("/nodes/0/commit/statusCheckRollup"));
        let (checks, next_checks) = parse_contexts(rollup.and_then(|r| r.get("contexts")));
        let summary = rollup.and_then(summarize_counts);
        ParsedPr {
            fields: PrFields {
                number: self.number,
                title: self.title,
                url: self.url,
                state: self.state,
                is_draft: self.is_draft,
                review_decision: self.review_decision,
                mergeable: self.mergeable,
                base_ref_name: self.base_ref_name,
                checks,
                summary,
            },
            next_checks,
        }
    }
}

/// The counts say failing/pending, but the checks we have don't include one.
fn missing_active_check(f: &PrFields) -> bool {
    match f.summary.map(|s| s.status) {
        Some(ChecksStatus::Failing) => !f.checks.iter().any(is_failed),
        Some(ChecksStatus::Pending) => !f.checks.iter().any(is_pending),
        _ => false,
    }
}

/// Pick the PR `gh pr view <branch>` would: same-repo PRs whose head is exactly
/// `branch`, OPEN first, otherwise the most recently created (nodes arrive
/// CREATED_AT DESC).
fn pick_pr(nodes: Vec<GqlPr>, branch: &str) -> Option<GqlPr> {
    let mut candidates: Vec<GqlPr> = nodes
        .into_iter()
        .filter(|p| !p.is_cross_repository && p.head_ref_name.as_deref().is_none_or(|h| h == branch))
        .collect();
    let open = candidates.iter().position(|p| p.state == "OPEN");
    match open {
        Some(i) => Some(candidates.swap_remove(i)),
        None if !candidates.is_empty() => Some(candidates.remove(0)),
        None => None,
    }
}

/// `rateLimit.remaining` from a GraphQL response, if present.
fn rate_remaining(stdout: &str) -> Option<u64> {
    let json: Value = serde_json::from_str(stdout.trim()).ok()?;
    json.pointer("/data/rateLimit/remaining").and_then(Value::as_u64)
}

/// Parse a GraphQL response for `branches`. `Err` means the response as a whole
/// is unusable (→ fall back to `gh pr view`).
fn parse_graphql(stdout: &str, branches: &[String]) -> Result<Vec<(String, FetchResult<ParsedPr>)>, String> {
    let json: Value = serde_json::from_str(stdout.trim()).map_err(|e| format!("unparseable gh graphql output: {e}"))?;
    let repo = json.pointer("/data/repository").filter(|v| v.is_object()).ok_or_else(|| {
        let msg = json.pointer("/errors/0/message").and_then(Value::as_str).unwrap_or("no repository in response");
        format!("graphql: {msg}")
    })?;
    // Per-alias errors (path = ["repository", "bN", …]).
    let mut alias_errors: Vec<(String, String)> = Vec::new();
    if let Some(errs) = json.get("errors").and_then(Value::as_array) {
        for e in errs {
            let alias = e.pointer("/path/1").and_then(Value::as_str);
            let msg = e.get("message").and_then(Value::as_str).unwrap_or("graphql error");
            match alias {
                Some(a) => alias_errors.push((a.to_string(), msg.to_string())),
                None => return Err(format!("graphql: {msg}")),
            }
        }
    }
    let mut out = Vec::with_capacity(branches.len());
    for (i, branch) in branches.iter().enumerate() {
        let alias = format!("b{i}");
        if let Some((_, msg)) = alias_errors.iter().find(|(a, _)| *a == alias) {
            out.push((branch.clone(), FetchResult::Err(format!("graphql: {msg}"))));
            continue;
        }
        let Some(conn) = repo.get(&alias) else {
            out.push((branch.clone(), FetchResult::Err(format!("graphql: missing {alias} in response"))));
            continue;
        };
        let nodes: Vec<GqlPr> = conn
            .get("nodes")
            .and_then(Value::as_array)
            .map(|ns| ns.iter().filter_map(|n| serde_json::from_value(n.clone()).ok()).collect())
            .unwrap_or_default();
        out.push((branch.clone(), FetchResult::Ok(pick_pr(nodes, branch).map(GqlPr::into_parsed))));
    }
    Ok(out)
}

/// Pure parse of one batched response (no follow-up pages). Exposed for tests
/// and tooling; [`fetch_prs`] is the normal entry point.
pub fn parse_graphql_response(stdout: &str, branches: &[String]) -> Result<Vec<(String, FetchResult<PrInfo>)>, String> {
    Ok(parse_graphql(stdout, branches)?
        .into_iter()
        .map(|(b, r)| {
            let r = match r {
                FetchResult::Ok(p) => FetchResult::Ok(p.map(|p| to_pr_info(p.fields))),
                FetchResult::Err(e) => FetchResult::Err(e),
            };
            (b, r)
        })
        .collect())
}

/// Page through more checks until the active one turns up. Best effort: a
/// failure here keeps the status from the counts and just leaves the active
/// check unnamed.
fn fill_active_check(gh: &dyn GhRunner, repo_path: &Path, pr: &mut ParsedPr, gh_calls: &mut usize) {
    let mut pages = 0;
    while missing_active_check(&pr.fields) && pages < EXTRA_CHECK_PAGES {
        let Some(cursor) = pr.next_checks.take() else { return };
        pages += 1;
        *gh_calls += 1;
        let out = gh.run(repo_path, &more_checks_args(pr.fields.number, &cursor), GRAPHQL_TIMEOUT);
        let json: Option<Value> = serde_json::from_str(out.stdout.trim()).ok();
        let contexts = json
            .as_ref()
            .and_then(|j| j.pointer("/data/repository/pullRequest/commits/nodes/0/commit/statusCheckRollup/contexts"));
        if contexts.is_none() {
            debug!("github", "extra checks page for PR #{} failed: {}", pr.fields.number, summarize_error(&out.stderr));
            return;
        }
        let (more, next) = parse_contexts(contexts);
        pr.fields.checks.extend(more);
        pr.next_checks = next;
    }
}

/// Fetch PR info for every branch of one repo: one `gh api graphql` per
/// [`GRAPHQL_CHUNK`] branches, falling back to per-branch `gh pr view` on a
/// non-network GraphQL failure. Stops at the first network-class failure.
pub fn fetch_prs(gh: &dyn GhRunner, repo_path: &Path, branches: &[String]) -> PrBatch {
    let mut seen = HashSet::new();
    let branches: Vec<String> = branches.iter().filter(|b| seen.insert(b.as_str())).cloned().collect();
    let mut batch =
        PrBatch { results: Vec::with_capacity(branches.len()), network_error: None, gh_calls: 0, rate_remaining: None };

    for chunk in branches.chunks(GRAPHQL_CHUNK) {
        if batch.network_error.is_some() {
            for b in chunk {
                batch.results.push((b.clone(), FetchResult::Err("aborted: network unavailable".into())));
            }
            continue;
        }
        batch.gh_calls += 1;
        let out = gh.run(repo_path, &graphql_args(chunk), GRAPHQL_TIMEOUT);
        if let Some(r) = rate_remaining(&out.stdout) {
            batch.rate_remaining = Some(r);
        }
        // gh exits 1 on GraphQL errors but still prints the body; try stdout first.
        let parsed = if out.stdout.trim().is_empty() {
            Err(if out.ok { "empty gh graphql output".to_string() } else { out.stderr.clone() })
        } else {
            parse_graphql(&out.stdout, chunk)
        };
        match parsed {
            Ok(results) => {
                for (branch, result) in results {
                    let result = match result {
                        FetchResult::Ok(Some(mut pr)) => {
                            fill_active_check(gh, repo_path, &mut pr, &mut batch.gh_calls);
                            FetchResult::Ok(Some(to_pr_info(pr.fields)))
                        }
                        FetchResult::Ok(None) => FetchResult::Ok(None),
                        FetchResult::Err(e) => FetchResult::Err(e),
                    };
                    batch.results.push((branch, result));
                }
            }
            Err(_) if !out.ok && is_network_error(&out.stderr) => {
                let err = summarize_error(&out.stderr);
                debug!("github", "gh graphql for {} failed (network): {err}", repo_path.display());
                batch.network_error = Some(err.clone());
                for b in chunk {
                    batch.results.push((b.clone(), FetchResult::Err(err.clone())));
                }
            }
            Err(e) => {
                debug!(
                    "github",
                    "gh graphql unusable for {} ({}), falling back to gh pr view per branch",
                    repo_path.display(),
                    summarize_error(&e)
                );
                for b in chunk {
                    if batch.network_error.is_some() {
                        batch.results.push((b.clone(), FetchResult::Err("aborted: network unavailable".into())));
                        continue;
                    }
                    batch.gh_calls += 1;
                    let (res, network) = fetch_pr_view(gh, repo_path, b);
                    if network && let FetchResult::Err(e) = &res {
                        batch.network_error = Some(e.clone());
                    }
                    batch.results.push((b.clone(), res));
                }
            }
        }
    }
    batch
}

/// The gh `pr view --json` shape (fallback path).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhPrView {
    number: u64,
    #[serde(default)]
    title: String,
    #[serde(default)]
    url: String,
    state: String,
    #[serde(default)]
    is_draft: bool,
    #[serde(default)]
    review_decision: Option<String>,
    #[serde(default)]
    mergeable: Option<String>,
    #[serde(default)]
    head_ref_name: Option<String>,
    #[serde(default)]
    base_ref_name: Option<String>,
    #[serde(default)]
    status_check_rollup: Option<Vec<Value>>,
}

/// Port of `fetchPrResult` (by branch name). Returns the result and whether a
/// failure was network-class.
pub fn fetch_pr_view(gh: &dyn GhRunner, repo_path: &Path, branch: &str) -> (FetchResult<PrInfo>, bool) {
    let args: Vec<String> =
        ["pr", "view", branch, "--json", GH_PR_FIELDS].iter().map(|s| s.to_string()).collect();
    let out = gh.run(repo_path, &args, PR_VIEW_TIMEOUT);
    if !out.ok {
        // Authoritative "no PR" answers, not API errors.
        if out.stderr.contains("no pull requests found") || out.stderr.contains("Could not resolve") {
            return (FetchResult::Ok(None), false);
        }
        let network = is_network_error(&out.stderr);
        return (FetchResult::Err(summarize_error(&out.stderr)), network);
    }
    let pr: GhPrView = match serde_json::from_str(out.stdout.trim()) {
        Ok(p) => p,
        Err(e) => return (FetchResult::Err(format!("unparseable gh pr view output: {e}")), false),
    };
    // gh can fuzzy-match a different branch; only an exact head match counts.
    if pr.head_ref_name.as_deref() != Some(branch) {
        debug!(
            "github",
            "PR #{} head branch {:?} does not match requested branch {branch:?}, ignoring",
            pr.number,
            pr.head_ref_name
        );
        return (FetchResult::Ok(None), false);
    }
    let checks = pr
        .status_check_rollup
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| serde_json::from_value::<RawCheck>(v).ok())
        .map(RawCheck::normalize)
        .collect();
    let info = to_pr_info(PrFields {
        number: pr.number,
        title: pr.title,
        url: pr.url,
        state: pr.state,
        is_draft: pr.is_draft,
        review_decision: pr.review_decision,
        mergeable: pr.mergeable,
        base_ref_name: pr.base_ref_name,
        checks,
        summary: None,
    });
    (FetchResult::Ok(Some(info)), false)
}

// ---------------------------------------------------------------------------
// Change detection with conditional REST requests
// ---------------------------------------------------------------------------
//
// GraphQL has no conditional requests, so every GraphQL poll costs points even
// when nothing changed. REST does: a `304 Not Modified` to an `If-None-Match`
// request "does not count against your primary rate limit" (GitHub docs,
// verified with gh's token). The [`Detector`] polls three cheap REST
// resources per repo and only reports the branches whose PR or CI changed;
// the caller then fetches just those with the batched GraphQL query above.

pub const REST_TIMEOUT: Duration = Duration::from_secs(10);
/// How often the detector polls when GitHub doesn't ask for less
/// (`X-Poll-Interval`).
pub const DETECT_EVERY: Duration = Duration::from_secs(20);
/// After a non-network failure (no REST via this gh, GHE quirk, the E2E shim)
/// the detector rests this long before trying again.
pub const DETECT_RETRY: Duration = Duration::from_secs(15 * 60);

/// One `gh api -i` response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestResponse {
    pub status: u16,
    pub etag: Option<String>,
    /// `X-Poll-Interval` in seconds, when GitHub sends one.
    pub poll_interval: Option<u64>,
    pub body: String,
}

/// Parse `gh api -i` output: `HTTP/2.0 200 OK`, headers, a blank line, body.
/// `None` when it doesn't start with a status line.
pub fn parse_include(stdout: &str) -> Option<RestResponse> {
    let text = stdout.trim_start();
    let (head, body) = match text.find("\r\n\r\n") {
        Some(i) => (&text[..i], &text[i + 4..]),
        None => match text.find("\n\n") {
            Some(i) => (&text[..i], &text[i + 2..]),
            None => (text, ""),
        },
    };
    let mut lines = head.lines().map(|l| l.trim_end_matches('\r'));
    let status_line = lines.next()?;
    if !status_line.starts_with("HTTP/") {
        return None;
    }
    let status = status_line.split_whitespace().nth(1)?.parse().ok()?;
    let (mut etag, mut poll_interval) = (None, None);
    for line in lines {
        let Some((k, v)) = line.split_once(':') else { continue };
        let v = v.trim();
        if k.eq_ignore_ascii_case("etag") {
            etag = Some(v.to_string());
        } else if k.eq_ignore_ascii_case("x-poll-interval") {
            poll_interval = v.parse().ok();
        }
    }
    Some(RestResponse { status, etag, poll_interval, body: body.to_string() })
}

/// Conditional `GET` through `gh api -i`. gh exits 1 on a 304, so the status
/// line, not the exit code, decides. `Err((message, is_network))`.
pub fn rest_get(gh: &dyn GhRunner, repo_path: &Path, path: &str, etag: Option<&str>) -> Result<RestResponse, (String, bool)> {
    let mut args: Vec<String> = vec!["api".into(), "-i".into()];
    if let Some(e) = etag {
        args.push("-H".into());
        args.push(format!("If-None-Match: {e}"));
    }
    args.push(path.into());
    let out = gh.run(repo_path, &args, REST_TIMEOUT);
    match parse_include(&out.stdout) {
        Some(r) => Ok(r),
        None if !out.ok && is_network_error(&out.stderr) => Err((summarize_error(&out.stderr), true)),
        None => {
            let why = if out.ok { "no HTTP status line in gh api -i output".to_string() } else { summarize_error(&out.stderr) };
            Err((why, false))
        }
    }
}

/// An open, same-repo PR as the detector last saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct OpenPr {
    updated_at: String,
    head_sha: String,
}

#[derive(Deserialize)]
struct RestPr {
    #[serde(default)]
    updated_at: String,
    head: RestRef,
    base: RestRef,
}

#[derive(Deserialize)]
struct RestRef {
    #[serde(rename = "ref")]
    name: String,
    sha: String,
    repo: Option<RestRepo>,
}

#[derive(Deserialize)]
struct RestRepo {
    full_name: String,
}

/// Open same-repo PRs by head branch, from a `GET /pulls` body.
fn parse_open_prs(body: &str) -> Result<std::collections::HashMap<String, OpenPr>, String> {
    let prs: Vec<RestPr> = serde_json::from_str(body.trim()).map_err(|e| format!("unparseable pulls list: {e}"))?;
    Ok(prs
        .into_iter()
        .filter(|p| match (&p.head.repo, &p.base.repo) {
            (Some(h), Some(b)) => h.full_name == b.full_name,
            _ => false,
        })
        .map(|p| (p.head.name, OpenPr { updated_at: p.updated_at, head_sha: p.head.sha }))
        .collect())
}

/// What one detector poll found.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Detection {
    /// Tracked branches whose PR or CI changed since the last poll.
    pub dirty: Vec<String>,
    /// gh calls made (all 304s cost no quota).
    pub calls: usize,
    /// Set when GitHub was unreachable; the poll stopped there.
    pub network_error: Option<String>,
}

/// Per-repo REST change detector (see the section comment above).
#[derive(Debug, Default)]
pub struct Detector {
    etags: std::collections::HashMap<String, String>,
    open: std::collections::HashMap<String, OpenPr>,
    primed: bool,
    next_poll: Option<Instant>,
    disabled_until: Option<Instant>,
}

impl Detector {
    /// Usable now: not resting after a failure. Before the first poll it counts
    /// as usable so the first tick tries it.
    pub fn is_active(&self, now: Instant) -> bool {
        self.disabled_until.is_none_or(|t| now >= t)
    }

    /// Whether at least one poll succeeded (its view of the repo is current).
    pub fn is_primed(&self) -> bool {
        self.primed && self.disabled_until.is_none()
    }

    pub fn is_due(&self, now: Instant) -> bool {
        self.is_active(now) && self.next_poll.is_none_or(|t| now >= t)
    }

    /// Poll the open-PR list, then each tracked open PR's check runs and
    /// commit statuses (CI finishing doesn't bump the PR's `updated_at`).
    pub fn poll(&mut self, gh: &dyn GhRunner, repo_path: &Path, tracked: &HashSet<String>, now: Instant) -> Detection {
        let mut d = Detection::default();
        let pulls = "repos/{owner}/{repo}/pulls?state=open&sort=updated&direction=desc&per_page=100";
        d.calls += 1;
        let r = match rest_get(gh, repo_path, pulls, self.etags.get(pulls).map(String::as_str)) {
            Ok(r) if r.status == 200 || r.status == 304 => r,
            Ok(r) => return self.disable(repo_path, now, &format!("HTTP {} for the pulls list", r.status), d),
            Err((e, true)) => {
                d.network_error = Some(e);
                return d;
            }
            Err((e, false)) => return self.disable(repo_path, now, &e, d),
        };
        let wait = r.poll_interval.map_or(DETECT_EVERY, |s| DETECT_EVERY.max(Duration::from_secs(s)));
        self.next_poll = Some(now + wait);
        if r.status == 200 {
            let open = match parse_open_prs(&r.body) {
                Ok(o) => o,
                Err(e) => return self.disable(repo_path, now, &e, d),
            };
            if self.primed {
                let changed = open
                    .iter()
                    .filter(|(b, p)| self.open.get(*b) != Some(p))
                    .map(|(b, _)| b)
                    .chain(self.open.keys().filter(|b| !open.contains_key(*b)));
                d.dirty.extend(changed.filter(|b| tracked.contains(*b)).cloned());
            }
            self.open = open;
            if let Some(e) = r.etag {
                self.etags.insert(pulls.to_string(), e);
            }
        }
        self.disabled_until = None;

        let mut wanted: HashSet<String> = HashSet::from([pulls.to_string()]);
        let mut heads: Vec<(&String, &OpenPr)> = self.open.iter().filter(|(b, _)| tracked.contains(*b)).collect();
        heads.sort_by_key(|(b, _)| *b);
        for (branch, pr) in heads {
            for path in [
                format!("repos/{{owner}}/{{repo}}/commits/{}/check-runs?per_page=100", pr.head_sha),
                format!("repos/{{owner}}/{{repo}}/commits/{}/status", pr.head_sha),
            ] {
                wanted.insert(path.clone());
                let prev = self.etags.get(&path).cloned();
                d.calls += 1;
                match rest_get(gh, repo_path, &path, prev.as_deref()) {
                    Ok(r) if r.status == 200 => {
                        // A first look at this commit only records the ETag:
                        // a new head already made the branch dirty above.
                        if prev.is_some() && !d.dirty.contains(branch) {
                            d.dirty.push(branch.clone());
                        }
                        if let Some(e) = r.etag {
                            self.etags.insert(path, e);
                        }
                    }
                    Ok(r) if r.status == 304 => {}
                    // e.g. a token without checks access: the PR list still works.
                    Ok(r) => debug!("github", "detector: HTTP {} for {path} in {}", r.status, repo_path.display()),
                    Err((e, true)) => {
                        d.network_error = Some(e);
                        return d;
                    }
                    Err((e, false)) => debug!("github", "detector: {path} failed in {}: {e}", repo_path.display()),
                }
            }
        }
        // Forget ETags of commits that are no longer an open PR's head.
        self.etags.retain(|p, _| wanted.contains(p));
        if !self.primed {
            self.primed = true;
            debug!("github", "detector primed for {}: {} open PR(s)", repo_path.display(), self.open.len());
        }
        d
    }

    fn disable(&mut self, repo_path: &Path, now: Instant, why: &str, d: Detection) -> Detection {
        if self.disabled_until.is_none() {
            info!("github", "PR change detection unavailable for {} ({why}); polling instead", repo_path.display());
        }
        self.disabled_until = Some(now + DETECT_RETRY);
        self.primed = false;
        self.etags.clear();
        self.open.clear();
        d
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    /// Scripted gh: pops one response per call and records the argv.
    #[derive(Clone, Default)]
    pub struct FakeGh {
        pub responses: Arc<Mutex<VecDeque<GhOutput>>>,
        pub calls: Arc<Mutex<Vec<Vec<String>>>>,
    }

    impl FakeGh {
        pub fn new(responses: Vec<GhOutput>) -> Self {
            FakeGh { responses: Arc::new(Mutex::new(responses.into())), calls: Arc::default() }
        }
        pub fn push(&self, r: GhOutput) {
            self.responses.lock().unwrap().push_back(r);
        }
        pub fn call_count(&self) -> usize {
            self.calls.lock().unwrap().len()
        }
    }

    impl GhRunner for FakeGh {
        fn run(&self, _cwd: &Path, args: &[String], _timeout: Duration) -> GhOutput {
            self.calls.lock().unwrap().push(args.to_vec());
            self.responses.lock().unwrap().pop_front().unwrap_or_else(|| GhOutput::failure("no scripted response"))
        }
    }

    fn check(status: &str, conclusion: &str) -> Check {
        Check { status: status.into(), conclusion: conclusion.into(), ..Default::default() }
    }

    fn pr(state: &str, review: &str, draft: bool, checks: ChecksStatus) -> PrInfo {
        PrInfo {
            number: 1,
            title: "t".into(),
            url: "u".into(),
            state: state.into(),
            is_draft: draft,
            review_decision: review.into(),
            has_feedback: false,
            checks_status: checks,
            active_check_url: None,
            active_check_name: None,
            checks_waiting: false,
            base_ref_name: None,
            mergeable: None,
        }
    }

    pub fn gql_node(number: u64, branch: &str, state: &str, checks: Value) -> Value {
        serde_json::json!({
            "number": number, "title": format!("PR {number}"), "url": format!("https://github.com/o/r/pull/{number}"),
            "state": state, "isDraft": false, "reviewDecision": null, "headRefName": branch,
            "isCrossRepository": false,
            "commits": {"nodes": [{"commit": {"statusCheckRollup": {"contexts": {"nodes": checks}}}}]}
        })
    }

    pub fn gql_response(per_branch: Vec<Vec<Value>>) -> String {
        let mut repo = serde_json::Map::new();
        for (i, nodes) in per_branch.into_iter().enumerate() {
            repo.insert(format!("b{i}"), serde_json::json!({ "nodes": nodes }));
        }
        serde_json::json!({"data": {"repository": repo}}).to_string()
    }

    fn branches(bs: &[&str]) -> Vec<String> {
        bs.iter().map(|s| s.to_string()).collect()
    }

    // --- deriveChecksStatus ---

    #[test]
    fn checks_status_none_for_empty() {
        assert_eq!(derive_checks_status(&[]), ChecksStatus::None);
    }

    #[test]
    fn checks_status_passing_when_all_completed() {
        assert_eq!(
            derive_checks_status(&[check("COMPLETED", "SUCCESS"), check("COMPLETED", "NEUTRAL")]),
            ChecksStatus::Passing
        );
    }

    #[test]
    fn checks_status_failing_for_failure_error_cancelled() {
        for c in ["FAILURE", "ERROR", "CANCELLED"] {
            assert_eq!(
                derive_checks_status(&[check("COMPLETED", "SUCCESS"), check("COMPLETED", c)]),
                ChecksStatus::Failing
            );
        }
    }

    #[test]
    fn checks_status_pending_when_in_progress() {
        assert_eq!(
            derive_checks_status(&[check("COMPLETED", "SUCCESS"), check("IN_PROGRESS", "")]),
            ChecksStatus::Pending
        );
    }

    #[test]
    fn checks_failure_takes_priority_over_pending() {
        assert_eq!(
            derive_checks_status(&[check("IN_PROGRESS", ""), check("COMPLETED", "FAILURE")]),
            ChecksStatus::Failing
        );
    }

    #[test]
    fn status_context_is_normalised() {
        let raw: RawCheck =
            serde_json::from_value(serde_json::json!({"__typename":"StatusContext","context":"ci/x","state":"SUCCESS","targetUrl":"t"}))
                .unwrap();
        let c = raw.normalize();
        assert_eq!(c.status, "COMPLETED");
        assert_eq!(c.conclusion, "SUCCESS");
        assert_eq!(c.name.as_deref(), Some("ci/x"));
        let raw: RawCheck =
            serde_json::from_value(serde_json::json!({"context":"ci/x","state":"PENDING"})).unwrap();
        assert!(is_pending(&raw.normalize()));
    }

    // --- getPrStatusLabel ---

    #[test]
    fn status_labels() {
        use ChecksStatus::*;
        let cases: Vec<(PrInfo, &str, &str)> = vec![
            (pr("MERGED", "", false, Passing), "Merged", "magenta"),
            (pr("MERGED", "", false, Failing), "Merged - Actions Failing", "red"),
            (pr("MERGED", "", false, Pending), "Merged - Actions Running", "magenta"),
            (pr("CLOSED", "", false, None), "Closed", "red"),
            (pr("OPEN", "CHANGES_REQUESTED", false, Passing), "Changes Requested", "red"),
            (pr("OPEN", "APPROVED", false, Passing), "Approved", "green"),
            (pr("OPEN", "APPROVED", false, Failing), "Approved - Checks Failing", "yellow"),
            (pr("OPEN", "APPROVED", false, Pending), "Approved - Checks Running", "cyan"),
            (pr("OPEN", "", true, None), "Draft", "gray"),
            (pr("OPEN", "REVIEW_REQUIRED", false, None), "In Review", "cyan"),
            (pr("OPEN", "", false, Failing), "In Review - Checks Failing", "yellow"),
            (pr("OPEN", "", false, Pending), "In Review - Checks Running", "cyan"),
        ];
        for (p, label, color) in cases {
            assert_eq!(pr_status_label(&p), (label, color), "{p:?}");
        }
        let mut conflicts = pr("OPEN", "APPROVED", false, Passing);
        conflicts.mergeable = Some("CONFLICTING".into());
        assert_eq!(pr_status_label(&conflicts), ("Approved - Conflicts", "yellow"));
        let mut draft_fb = pr("OPEN", "", true, None);
        draft_fb.has_feedback = true;
        assert_eq!(pr_status_label(&draft_fb), ("Draft - Feedback", "yellow"));
        let mut fb = pr("OPEN", "", false, None);
        fb.has_feedback = true;
        assert_eq!(pr_status_label(&fb), ("In Review - Feedback", "yellow"));
        let mut waiting = pr("MERGED", "", false, Pending);
        waiting.checks_waiting = true;
        assert_eq!(pr_status_label(&waiting), ("Merged - Awaiting Approval", "yellow"));
        let mut fail_waiting = pr("MERGED", "", false, Failing);
        fail_waiting.checks_waiting = true;
        assert_eq!(pr_status_label(&fail_waiting).0, "Merged - Actions Failing");
    }

    // --- GraphQL batch ---

    #[test]
    fn graphql_args_pass_branches_as_variables() {
        let args = graphql_args(&branches(&["feat/a\"; drop", "b"]));
        assert_eq!(&args[..6], &["api", "graphql", "-F", "owner={owner}", "-F", "name={repo}"]);
        assert!(args.contains(&"b0=feat/a\"; drop".to_string()));
        assert!(args.contains(&"b1=b".to_string()));
        let q = args.last().unwrap();
        assert!(q.contains("b1: pullRequests(headRefName: $b1"));
        assert!(!q.contains("feat/a"));
    }

    #[test]
    fn one_graphql_call_fetches_every_branch() {
        let checks = serde_json::json!([
            {"__typename":"CheckRun","name":"build","status":"COMPLETED","conclusion":"SUCCESS","detailsUrl":"d1"},
            {"__typename":"CheckRun","name":"deploy","status":"WAITING","conclusion":null,"detailsUrl":"d2"}
        ]);
        let body = gql_response(vec![vec![gql_node(7, "a", "OPEN", checks)], vec![]]);
        let gh = FakeGh::new(vec![GhOutput::success(body)]);
        let batch = fetch_prs(&gh, Path::new("/r"), &branches(&["a", "b", "a"]));
        assert_eq!(gh.call_count(), 1);
        assert_eq!(batch.gh_calls, 1);
        assert!(batch.network_error.is_none());
        assert_eq!(batch.results.len(), 2, "duplicate branches are deduped");
        let FetchResult::Ok(Some(a)) = &batch.results[0].1 else { panic!("{:?}", batch.results[0]) };
        assert_eq!(a.number, 7);
        assert_eq!(a.checks_status, ChecksStatus::Pending);
        assert!(a.checks_waiting);
        assert_eq!(a.active_check_name.as_deref(), Some("deploy"));
        assert_eq!(a.active_check_url.as_deref(), Some("d2"));
        assert_eq!(a.review_decision, "");
        assert!(matches!(batch.results[1].1, FetchResult::Ok(None)));
    }

    #[test]
    fn prefers_open_pr_then_most_recent() {
        let none = serde_json::json!([]);
        let body = gql_response(vec![
            vec![gql_node(9, "a", "CLOSED", none.clone()), gql_node(5, "a", "OPEN", none.clone())],
            vec![gql_node(8, "b", "MERGED", none.clone()), gql_node(3, "b", "CLOSED", none.clone())],
        ]);
        let gh = FakeGh::new(vec![GhOutput::success(body)]);
        let batch = fetch_prs(&gh, Path::new("/r"), &branches(&["a", "b"]));
        let nums: Vec<u64> = batch
            .results
            .iter()
            .map(|(_, r)| match r {
                FetchResult::Ok(Some(p)) => p.number,
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(nums, vec![5, 8]);
    }

    #[test]
    fn ignores_cross_repository_prs() {
        let mut fork = gql_node(4, "main", "OPEN", serde_json::json!([]));
        fork["isCrossRepository"] = true.into();
        let gh = FakeGh::new(vec![GhOutput::success(gql_response(vec![vec![fork]]))]);
        let batch = fetch_prs(&gh, Path::new("/r"), &branches(&["main"]));
        assert!(matches!(batch.results[0].1, FetchResult::Ok(None)));
    }

    #[test]
    fn failing_check_is_the_active_one() {
        let checks = serde_json::json!([
            {"__typename":"CheckRun","name":"lint","status":"IN_PROGRESS","conclusion":null,"detailsUrl":"p"},
            {"__typename":"CheckRun","name":"test","status":"COMPLETED","conclusion":"FAILURE","detailsUrl":"f"}
        ]);
        let gh = FakeGh::new(vec![GhOutput::success(gql_response(vec![vec![gql_node(1, "a", "OPEN", checks)]]))]);
        let batch = fetch_prs(&gh, Path::new("/r"), &branches(&["a"]));
        let FetchResult::Ok(Some(p)) = &batch.results[0].1 else { panic!() };
        assert_eq!(p.checks_status, ChecksStatus::Failing);
        assert_eq!(p.active_check_name.as_deref(), Some("test"));
    }

    fn counts(check_runs: &[(&str, u64)], statuses: &[(&str, u64)]) -> (Value, Value) {
        let to = |xs: &[(&str, u64)]| -> Value {
            xs.iter().map(|(s, c)| serde_json::json!({"state": s, "count": c})).collect()
        };
        (to(check_runs), to(statuses))
    }

    /// A PR whose first page of checks is all green but whose counts (over all
    /// 357 checks) include failures — the real case that motivated using counts.
    #[test]
    fn counts_beat_a_truncated_first_page_and_more_pages_name_the_failure() {
        let mut node = gql_node(
            18671,
            "a",
            "OPEN",
            serde_json::json!([{"__typename":"CheckRun","name":"ok","status":"COMPLETED","conclusion":"SUCCESS"}]),
        );
        let (cr, sc) = counts(&[("SUCCESS", 112), ("SKIPPED", 237), ("FAILURE", 6)], &[("SUCCESS", 2)]);
        let ctx = &mut node["commits"]["nodes"][0]["commit"]["statusCheckRollup"]["contexts"];
        ctx["checkRunCountsByState"] = cr;
        ctx["statusContextCountsByState"] = sc;
        ctx["pageInfo"] = serde_json::json!({"hasNextPage": true, "endCursor": "CUR1"});
        let page2 = serde_json::json!({"data": {"repository": {"pullRequest": {"commits": {"nodes": [{"commit": {
            "statusCheckRollup": {"contexts": {
                "pageInfo": {"hasNextPage": false, "endCursor": null},
                "nodes": [{"__typename":"CheckRun","name":"e2e","status":"COMPLETED","conclusion":"FAILURE","detailsUrl":"f"}]
            }}}}]}}}}});
        let gh = FakeGh::new(vec![
            GhOutput::success(gql_response(vec![vec![node]])),
            GhOutput::success(page2.to_string()),
        ]);
        let batch = fetch_prs(&gh, Path::new("/r"), &branches(&["a"]));
        assert_eq!(gh.call_count(), 2);
        let calls = gh.calls.lock().unwrap();
        assert!(calls[1].contains(&"n=18671".to_string()) && calls[1].contains(&"after=CUR1".to_string()));
        drop(calls);
        let FetchResult::Ok(Some(p)) = &batch.results[0].1 else { panic!() };
        assert_eq!(p.checks_status, ChecksStatus::Failing);
        assert_eq!(p.active_check_name.as_deref(), Some("e2e"));
    }

    #[test]
    fn counts_alone_decide_status_without_extra_calls_when_nothing_is_missing() {
        let mut node = gql_node(1, "a", "OPEN", serde_json::json!([]));
        let (cr, sc) = counts(&[("SUCCESS", 3), ("ACTION_REQUIRED", 1)], &[("SUCCESS", 2)]);
        let ctx = &mut node["commits"]["nodes"][0]["commit"]["statusCheckRollup"]["contexts"];
        ctx["checkRunCountsByState"] = cr;
        ctx["statusContextCountsByState"] = sc;
        ctx["pageInfo"] = serde_json::json!({"hasNextPage": true, "endCursor": "CUR1"});
        let gh = FakeGh::new(vec![GhOutput::success(gql_response(vec![vec![node]]))]);
        let batch = fetch_prs(&gh, Path::new("/r"), &branches(&["a"]));
        assert_eq!(gh.call_count(), 1);
        let FetchResult::Ok(Some(p)) = &batch.results[0].1 else { panic!() };
        assert_eq!(p.checks_status, ChecksStatus::Passing);
        assert!(p.checks_waiting);
        assert_eq!(p.active_check_name, None);
    }

    #[test]
    fn network_failure_aborts_without_fallback() {
        let gh = FakeGh::new(vec![GhOutput::failure(
            "error connecting to api.github.com\ncheck your internet connection or https://githubstatus.com\n",
        )]);
        let batch = fetch_prs(&gh, Path::new("/r"), &branches(&["a", "b"]));
        assert_eq!(gh.call_count(), 1, "no per-branch fallback while offline");
        assert_eq!(batch.network_error.as_deref(), Some("error connecting to api.github.com"));
        assert!(batch.results.iter().all(|(_, r)| matches!(r, FetchResult::Err(_))));
    }

    #[test]
    fn non_network_graphql_failure_falls_back_to_pr_view() {
        let view = serde_json::json!({
            "number": 42, "title": "t", "url": "u", "state": "OPEN", "isDraft": true,
            "reviewDecision": "", "headRefName": "a", "statusCheckRollup": []
        });
        let gh = FakeGh::new(vec![
            // e.g. the E2E shim before it learned graphql, or an old gh
            GhOutput::success(""),
            GhOutput::success(view.to_string()),
            GhOutput::failure("no pull requests found for branch \"b\""),
        ]);
        let batch = fetch_prs(&gh, Path::new("/r"), &branches(&["a", "b"]));
        assert_eq!(gh.call_count(), 3);
        assert_eq!(gh.calls.lock().unwrap()[1][..3], ["pr", "view", "a"]);
        let FetchResult::Ok(Some(p)) = &batch.results[0].1 else { panic!("{:?}", batch.results) };
        assert!(p.is_draft);
        assert_eq!(p.checks_status, ChecksStatus::None);
        assert!(matches!(batch.results[1].1, FetchResult::Ok(None)));
    }

    #[test]
    fn fallback_stops_at_first_network_failure() {
        let gh = FakeGh::new(vec![
            GhOutput::failure("gh: Something went wrong with the query"),
            GhOutput::failure("dial tcp: lookup api.github.com: no such host"),
        ]);
        let batch = fetch_prs(&gh, Path::new("/r"), &branches(&["a", "b", "c"]));
        assert_eq!(gh.call_count(), 2);
        assert!(batch.network_error.is_some());
        assert_eq!(batch.results.len(), 3);
        assert!(batch.results.iter().all(|(_, r)| matches!(r, FetchResult::Err(_))));
    }

    #[test]
    fn graphql_errors_on_one_alias_fail_only_that_branch() {
        let body = serde_json::json!({
            "data": {"repository": {"b0": {"nodes": []}, "b1": null}},
            "errors": [{"path": ["repository", "b1"], "message": "boom"}]
        });
        let gh = FakeGh::new(vec![GhOutput { ok: false, stdout: body.to_string(), stderr: "gh: boom".into() }]);
        let batch = fetch_prs(&gh, Path::new("/r"), &branches(&["a", "b"]));
        assert_eq!(gh.call_count(), 1);
        assert!(matches!(batch.results[0].1, FetchResult::Ok(None)));
        assert!(matches!(batch.results[1].1, FetchResult::Err(_)));
    }

    #[test]
    fn chunks_large_branch_lists() {
        let n = GRAPHQL_CHUNK + 3;
        let names: Vec<String> = (0..n).map(|i| format!("br{i}")).collect();
        let gh = FakeGh::new(vec![
            GhOutput::success(gql_response(vec![vec![]; GRAPHQL_CHUNK])),
            GhOutput::success(gql_response(vec![vec![]; 3])),
        ]);
        let batch = fetch_prs(&gh, Path::new("/r"), &names);
        assert_eq!(gh.call_count(), 2);
        assert_eq!(batch.results.len(), n);
        assert!(batch.results.iter().all(|(_, r)| matches!(r, FetchResult::Ok(None))));
    }

    // --- fetchPrResult (pr view fallback) ---

    #[test]
    fn pr_view_ignores_fuzzy_match_on_another_branch() {
        let view = serde_json::json!({"number": 1, "title": "t", "url": "u", "state": "OPEN", "headRefName": "other"});
        let gh = FakeGh::new(vec![GhOutput::success(view.to_string())]);
        let (r, net) = fetch_pr_view(&gh, Path::new("/r"), "mine");
        assert!(matches!(r, FetchResult::Ok(None)));
        assert!(!net);
    }

    #[test]
    fn pr_view_reports_failure_when_gh_cannot_reach_the_api() {
        let gh = FakeGh::new(vec![GhOutput::failure("error connecting to api.github.com")]);
        let (r, net) = fetch_pr_view(&gh, Path::new("/r"), "x");
        assert!(matches!(r, FetchResult::Err(_)));
        assert!(net);
    }

    // --- REST change detector ---

    fn http(status: &str, etag: &str, body: &str) -> GhOutput {
        let out = format!("HTTP/2.0 {status}\r\nEtag: {etag}\r\nX-Poll-Interval: 60\r\n\r\n{body}");
        // gh exits 1 on a 304 but still prints the headers.
        GhOutput { ok: status.starts_with('2'), stdout: out, stderr: String::new() }
    }

    fn pulls(prs: &[(&str, &str, &str)]) -> String {
        let v: Vec<Value> = prs
            .iter()
            .map(|(branch, sha, updated)| {
                serde_json::json!({
                    "updated_at": updated,
                    "head": {"ref": branch, "sha": sha, "repo": {"full_name": "o/r"}},
                    "base": {"ref": "main", "sha": "base", "repo": {"full_name": "o/r"}}
                })
            })
            .collect();
        Value::Array(v).to_string()
    }

    #[test]
    fn parses_gh_api_include_output() {
        let r = parse_include("HTTP/2.0 304 Not Modified\nEtag: W/\"abc\"\nX-Poll-Interval: 60\n\n").unwrap();
        assert_eq!((r.status, r.etag.as_deref(), r.poll_interval), (304, Some("W/\"abc\""), Some(60)));
        let r = parse_include("HTTP/1.1 200 OK\r\netag: \"x\"\r\n\r\n[1]").unwrap();
        assert_eq!((r.status, r.body.as_str()), (200, "[1]"));
        assert_eq!(parse_include(""), None);
        assert_eq!(parse_include("[]"), None);
    }

    #[test]
    fn detector_reports_only_what_changed() {
        let gh = FakeGh::default();
        let tracked: HashSet<String> = ["feat/a".to_string(), "feat/b".to_string()].into();
        let mut d = Detector::default();
        let t0 = Instant::now();
        assert!(d.is_due(t0));

        // First poll primes: PR list + feat/a's checks and statuses; nothing is dirty yet.
        gh.push(http("200 OK", "p1", &pulls(&[("feat/a", "s1", "t1"), ("other", "s9", "t1")])));
        gh.push(http("200 OK", "c1", "{}"));
        gh.push(http("200 OK", "st1", "{}"));
        let r = d.poll(&gh, Path::new("/r"), &tracked, t0);
        assert_eq!((r.dirty.len(), r.calls), (0, 3), "untracked PRs aren't polled for checks");
        assert!(!d.is_due(t0 + DETECT_EVERY), "X-Poll-Interval (60 s) is honoured");
        assert!(d.is_due(t0 + Duration::from_secs(60)));

        // CI moved on feat/a: only its check runs changed.
        gh.push(http("304 Not Modified", "p1", ""));
        gh.push(http("200 OK", "c2", "{}"));
        gh.push(http("304 Not Modified", "st1", ""));
        let r = d.poll(&gh, Path::new("/r"), &tracked, t0);
        assert_eq!(r.dirty, vec!["feat/a"]);
        let calls = gh.calls.lock().unwrap().clone();
        assert!(calls[3].contains(&"If-None-Match: p1".to_string()), "{:?}", calls[3]);

        // Nothing changed: all 304s.
        for e in ["p1", "c2", "st1"] {
            gh.push(http("304 Not Modified", e, ""));
        }
        assert!(d.poll(&gh, Path::new("/r"), &tracked, t0).dirty.is_empty());

        // feat/a closed or merged (left the open list), feat/b opened.
        gh.push(http("200 OK", "p2", &pulls(&[("feat/b", "s5", "t2")])));
        gh.push(http("200 OK", "c5", "{}"));
        gh.push(http("200 OK", "st5", "{}"));
        let mut dirty = d.poll(&gh, Path::new("/r"), &tracked, t0).dirty;
        dirty.sort();
        assert_eq!(dirty, vec!["feat/a", "feat/b"]);
    }

    #[test]
    fn detector_rests_after_a_non_network_failure() {
        let tracked = HashSet::new();
        let mut d = Detector::default();
        let t0 = Instant::now();
        // The E2E shim (and old setups) print nothing for `gh api -i`.
        let gh = FakeGh::new(vec![GhOutput::success("")]);
        d.poll(&gh, Path::new("/r"), &tracked, t0);
        assert!(!d.is_active(t0 + Duration::from_secs(60)));
        assert!(!d.is_primed());
        assert!(d.is_active(t0 + DETECT_RETRY));

        // Offline is not a reason to rest: try again next time.
        let mut d = Detector::default();
        let gh = FakeGh::new(vec![GhOutput::failure("error connecting to api.github.com")]);
        let r = d.poll(&gh, Path::new("/r"), &tracked, t0);
        assert!(r.network_error.is_some());
        assert!(d.is_due(t0));
    }

    #[test]
    fn system_gh_reports_spawn_failure_and_timeout() {
        // A directory that doesn't exist makes spawn fail regardless of gh being installed.
        let out = SystemGh.run(Path::new("/definitely/not/here"), &["--version".into()], Duration::from_secs(1));
        assert!(!out.ok);
        assert!(out.stderr.contains("failed to run gh"));
    }
}
