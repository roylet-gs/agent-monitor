//! Linear ticket lookup over raw GraphQL (port of `src/lib/linear.ts`).
//!
//! `issueVcsBranchSearch` is workspace-wide, so callers dedupe per distinct
//! branch. Unlike the TS app (one request per branch, all in parallel) the
//! branches are batched into one request with aliases (`b0: issueVcsBranchSearch(…)`),
//! chunked at [`LINEAR_CHUNK`] to stay inside Linear's query-complexity limit.
//!
//! Failure rules match the TS `FetchResult` contract: a non-2xx status, an
//! unparseable body, or a GraphQL `errors` entry is a failure — never "this
//! branch has no ticket". An error whose `path` names one alias fails only that
//! branch; any other error fails the whole chunk.
//!
//! The endpoint honours `AM_LINEAR_API_URL` (the Docker E2E mock).

use std::collections::HashSet;
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;

use super::FetchResult;
use super::breaker::{is_network_error, summarize_error};
use crate::debug;
use crate::model::{ChecksStatus, LinearInfo, LinearPrAttachment, LinearProject, LinearState, PrInfo};

pub const DEFAULT_LINEAR_API_URL: &str = "https://api.linear.app/graphql";
pub const LINEAR_CHUNK: usize = 25;
pub const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

/// `$AM_LINEAR_API_URL` or the public endpoint.
pub fn linear_api_url() -> String {
    std::env::var("AM_LINEAR_API_URL").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| DEFAULT_LINEAR_API_URL.into())
}

// ---------------------------------------------------------------------------
// HTTP transport (injectable)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

/// A request that never got an HTTP answer. `network` marks DNS/connect/timeout
/// failures, which open the circuit breaker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportError {
    pub network: bool,
    pub message: String,
}

/// Blocking JSON POST. Non-2xx statuses are returned as `Ok`, not `Err`.
pub trait HttpTransport: Send {
    fn post_json(&self, url: &str, headers: &[(&str, &str)], body: &str) -> Result<HttpResponse, TransportError>;
}

/// `ureq` with rustls and a global timeout.
pub struct UreqTransport {
    agent: ureq::Agent,
}

impl UreqTransport {
    pub fn new(timeout: Duration) -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .http_status_as_error(false)
            .build()
            .into();
        UreqTransport { agent }
    }
}

impl Default for UreqTransport {
    fn default() -> Self {
        Self::new(HTTP_TIMEOUT)
    }
}

impl HttpTransport for UreqTransport {
    fn post_json(&self, url: &str, headers: &[(&str, &str)], body: &str) -> Result<HttpResponse, TransportError> {
        let mut req = self.agent.post(url).header("Content-Type", "application/json");
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let classify = |e: ureq::Error| {
            let network = matches!(
                e,
                ureq::Error::Io(_) | ureq::Error::Timeout(_) | ureq::Error::HostNotFound | ureq::Error::ConnectionFailed
            ) || is_network_error(&e.to_string());
            TransportError { network, message: e.to_string() }
        };
        let mut resp = req.send(body).map_err(classify)?;
        let status = resp.status().as_u16();
        let body = resp.body_mut().read_to_string().map_err(classify)?;
        Ok(HttpResponse { status, body })
    }
}

// ---------------------------------------------------------------------------
// Fetch
// ---------------------------------------------------------------------------

const ISSUE_FIELDS: &str = "identifier title url state { name color type } priorityLabel assignee { name } \
     project { id name color url } attachments { nodes { url title sourceType metadata } }";

fn build_query(n: usize) -> String {
    let vars: Vec<String> = (0..n).map(|i| format!("$b{i}: String!")).collect();
    let body: String =
        (0..n).map(|i| format!(" b{i}: issueVcsBranchSearch(branchName: $b{i}) {{ ...issue }}")).collect();
    format!("query({}) {{{body} }}\nfragment issue on Issue {{ {ISSUE_FIELDS} }}", vars.join(", "))
}

/// Request body for one chunk; branches travel as variables.
pub fn request_body(branches: &[String]) -> String {
    let mut vars = serde_json::Map::new();
    for (i, b) in branches.iter().enumerate() {
        vars.insert(format!("b{i}"), Value::String(b.clone()));
    }
    serde_json::json!({ "query": build_query(branches.len()), "variables": vars }).to_string()
}

#[derive(Debug)]
pub struct LinearBatch {
    /// One entry per distinct requested branch, in request order.
    pub results: Vec<(String, FetchResult<LinearInfo>)>,
    /// Set when a network-class failure aborted the batch.
    pub network_error: Option<String>,
    pub requests: usize,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawIssue {
    identifier: String,
    title: String,
    url: String,
    state: LinearState,
    #[serde(default)]
    priority_label: Option<String>,
    #[serde(default)]
    assignee: Option<RawAssignee>,
    #[serde(default)]
    project: Option<LinearProject>,
    #[serde(default)]
    attachments: Option<RawAttachments>,
}

#[derive(Deserialize)]
struct RawAssignee {
    name: Option<String>,
}

#[derive(Deserialize)]
struct RawAttachments {
    #[serde(default)]
    nodes: Vec<RawAttachment>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawAttachment {
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    source_type: Option<String>,
    #[serde(default)]
    metadata: Value,
}

fn to_linear_info(issue: RawIssue) -> LinearInfo {
    // First GitHub PR attachment, used as a PR fallback when gh has nothing.
    let pr_attachment = issue.attachments.map(|a| a.nodes).unwrap_or_default().into_iter().find(|a| {
        a.source_type.as_deref().is_some_and(|s| s.to_lowercase().contains("github"))
            || a.url.as_deref().is_some_and(|u| u.contains("/pull/"))
    });
    LinearInfo {
        identifier: issue.identifier,
        title: issue.title,
        url: issue.url,
        state: issue.state,
        priority_label: issue.priority_label.unwrap_or_default(),
        assignee: issue.assignee.and_then(|a| a.name),
        project: issue.project,
        pr_attachment: pr_attachment.map(|a| LinearPrAttachment {
            url: a.url.unwrap_or_default(),
            title: a.title.unwrap_or_default(),
            metadata: a.metadata,
        }),
    }
}

/// Parse one chunk's 200 body. `Err` fails the whole chunk.
fn parse_chunk(body: &str, branches: &[String]) -> Result<Vec<(String, FetchResult<LinearInfo>)>, String> {
    let json: Value = serde_json::from_str(body).map_err(|e| format!("unparseable Linear response: {e}"))?;
    let mut alias_errors: Vec<(String, String)> = Vec::new();
    if let Some(errs) = json.get("errors").and_then(Value::as_array).filter(|e| !e.is_empty()) {
        for e in errs {
            let msg = e.get("message").and_then(Value::as_str).unwrap_or("unknown GraphQL error");
            match e.pointer("/path/0").and_then(Value::as_str) {
                Some(alias) if alias.starts_with('b') => alias_errors.push((alias.to_string(), msg.to_string())),
                _ => return Err(msg.to_string()),
            }
        }
    }
    let data = json.get("data").filter(|d| d.is_object()).ok_or_else(|| "Linear response has no data".to_string())?;
    let mut out = Vec::with_capacity(branches.len());
    for (i, branch) in branches.iter().enumerate() {
        let alias = format!("b{i}");
        if let Some((_, msg)) = alias_errors.iter().find(|(a, _)| *a == alias) {
            out.push((branch.clone(), FetchResult::Err(msg.clone())));
            continue;
        }
        let result = match data.get(&alias) {
            None => FetchResult::Err(format!("Linear response missing {alias}")),
            Some(Value::Null) => FetchResult::Ok(None),
            Some(v) => match serde_json::from_value::<RawIssue>(v.clone()) {
                Ok(issue) => FetchResult::Ok(Some(to_linear_info(issue))),
                Err(e) => FetchResult::Err(format!("unexpected Linear issue shape: {e}")),
            },
        };
        out.push((branch.clone(), result));
    }
    Ok(out)
}

/// Pull the GraphQL error message out of a body, if there is one.
fn graphql_message(body: &str) -> Option<String> {
    let v: Value = serde_json::from_str(body).ok()?;
    v.pointer("/errors/0/message").and_then(Value::as_str).map(str::to_string)
}

fn http_error(status: u16, body: &str) -> String {
    let detail = graphql_message(body).unwrap_or_else(|| body.chars().take(200).collect());
    summarize_error(&format!("HTTP {status}: {detail}"))
}

/// Fetch tickets for `branches` (deduped), batched. Stops at the first
/// network-class failure (including 5xx/429 — the service is unavailable).
pub fn fetch_linear_batch(http: &dyn HttpTransport, url: &str, api_key: &str, branches: &[String]) -> LinearBatch {
    let mut seen = HashSet::new();
    let branches: Vec<String> = branches.iter().filter(|b| seen.insert(b.as_str())).cloned().collect();
    let mut batch = LinearBatch { results: Vec::with_capacity(branches.len()), network_error: None, requests: 0 };
    let headers = [("Authorization", api_key)];

    for chunk in branches.chunks(LINEAR_CHUNK) {
        if let Some(err) = &batch.network_error {
            let err = err.clone();
            batch.results.extend(chunk.iter().map(|b| (b.clone(), FetchResult::Err(err.clone()))));
            continue;
        }
        batch.requests += 1;
        let outcome: Result<Vec<_>, (String, bool)> = match http.post_json(url, &headers, &request_body(chunk)) {
            Err(e) => Err((summarize_error(&e.message), e.network)),
            Ok(r) if !(200..300).contains(&r.status) => {
                let unavailable = r.status >= 500 || r.status == 429;
                Err((http_error(r.status, &r.body), unavailable))
            }
            Ok(r) => parse_chunk(&r.body, chunk).map_err(|e| (summarize_error(&e), false)),
        };
        match outcome {
            Ok(results) => batch.results.extend(results),
            Err((err, network)) => {
                debug!("linear", "Linear fetch for {} branch(es) failed: {err}", chunk.len());
                if network {
                    batch.network_error = Some(err.clone());
                }
                batch.results.extend(chunk.iter().map(|b| (b.clone(), FetchResult::Err(err.clone()))));
            }
        }
    }
    batch
}

/// Single-branch port of `fetchLinearResult`.
pub fn fetch_linear_result(http: &dyn HttpTransport, url: &str, api_key: &str, branch: &str) -> FetchResult<LinearInfo> {
    let mut batch = fetch_linear_batch(http, url, api_key, &[branch.to_string()]);
    batch.results.pop().map(|(_, r)| r).unwrap_or_else(|| FetchResult::Err("no result".into()))
}

/// Port of `verifyLinearApiKey`: the viewer's name, or Linear's own message.
pub fn verify_api_key(http: &dyn HttpTransport, url: &str, api_key: &str) -> Result<String, String> {
    let body = serde_json::json!({ "query": "{ viewer { name email } }" }).to_string();
    match http.post_json(url, &[("Authorization", api_key)], &body) {
        Err(e) => Err(summarize_error(&e.message)),
        Ok(r) if !(200..300).contains(&r.status) => {
            // A bad key gets a 401 whose body carries the real message.
            Err(graphql_message(&r.body).unwrap_or_else(|| format!("HTTP {}", r.status)))
        }
        Ok(r) => {
            let v: Value = serde_json::from_str(&r.body).map_err(|e| e.to_string())?;
            if let Some(name) = v.pointer("/data/viewer/name").and_then(Value::as_str) {
                return Ok(name.to_string());
            }
            Err(v.pointer("/errors/0/message").and_then(Value::as_str).unwrap_or("Invalid API key").to_string())
        }
    }
}

// ---------------------------------------------------------------------------
// PR attachment fallback
// ---------------------------------------------------------------------------

/// Port of `linearAttachmentMatchesBranch`: false only when the attachment
/// names a *different* branch.
pub fn attachment_matches_branch(attachment: &LinearPrAttachment, branch: &str) -> bool {
    match attachment.metadata.get("branch").and_then(Value::as_str) {
        Some(b) if !b.is_empty() && b != branch => {
            debug!("linear", "PR attachment branch {b:?} does not match worktree branch {branch:?}, ignoring");
            false
        }
        _ => true,
    }
}

/// Port of `linearAttachmentToPrInfo`.
pub fn attachment_to_pr_info(attachment: &LinearPrAttachment) -> PrInfo {
    let meta = &attachment.metadata;
    let present = |k: &str| meta.get(k).is_some_and(|v| !v.is_null() && v.as_str() != Some(""));
    let state = if present("mergedAt") {
        "MERGED"
    } else if present("closedAt") {
        "CLOSED"
    } else {
        "OPEN"
    };
    PrInfo {
        number: meta.get("number").and_then(Value::as_u64).unwrap_or(0),
        title: attachment.title.clone(),
        url: attachment.url.clone(),
        state: state.into(),
        is_draft: meta.get("draft").and_then(Value::as_bool).unwrap_or(false),
        review_decision: String::new(),
        has_feedback: false,
        checks_status: ChecksStatus::None,
        active_check_url: None,
        active_check_name: None,
        checks_waiting: false,
    }
}

/// Port of `getLinearStatusColor`.
pub fn linear_status_color(state_type: &str) -> &'static str {
    match state_type {
        "started" => "cyan",
        "completed" => "green",
        "canceled" => "red",
        _ => "gray",
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    /// Scripted transport: pops one response per request, records bodies.
    #[derive(Clone, Default)]
    pub struct FakeHttp {
        pub responses: Arc<Mutex<VecDeque<Result<HttpResponse, TransportError>>>>,
        pub bodies: Arc<Mutex<Vec<String>>>,
    }

    impl FakeHttp {
        pub fn new(responses: Vec<Result<HttpResponse, TransportError>>) -> Self {
            FakeHttp { responses: Arc::new(Mutex::new(responses.into())), bodies: Arc::default() }
        }
        pub fn push(&self, r: Result<HttpResponse, TransportError>) {
            self.responses.lock().unwrap().push_back(r);
        }
        pub fn request_count(&self) -> usize {
            self.bodies.lock().unwrap().len()
        }
    }

    impl HttpTransport for FakeHttp {
        fn post_json(&self, _url: &str, headers: &[(&str, &str)], body: &str) -> Result<HttpResponse, TransportError> {
            assert!(headers.iter().any(|(k, _)| *k == "Authorization"));
            self.bodies.lock().unwrap().push(body.to_string());
            self.responses.lock().unwrap().pop_front().unwrap_or_else(|| {
                Err(TransportError { network: false, message: "no scripted response".into() })
            })
        }
    }

    pub fn ok(body: Value) -> Result<HttpResponse, TransportError> {
        Ok(HttpResponse { status: 200, body: body.to_string() })
    }

    pub fn status(code: u16, body: Value) -> Result<HttpResponse, TransportError> {
        Ok(HttpResponse { status: code, body: body.to_string() })
    }

    pub fn offline() -> Result<HttpResponse, TransportError> {
        Err(TransportError { network: true, message: "getaddrinfo ENOTFOUND api.linear.app".into() })
    }

    pub fn issue(id: &str) -> Value {
        serde_json::json!({
            "identifier": id, "title": "Ticket", "url": format!("https://linear.app/team/issue/{id}"),
            "state": {"name": "In Progress", "color": "#0ea5e9", "type": "started"},
            "priorityLabel": "High", "assignee": null,
            "project": {"id": "proj-1", "name": "Dashboard Revamp", "color": "#5e6ad2", "url": "https://linear.app/p"},
            "attachments": {"nodes": [{
                "url": "https://github.com/test/repo/pull/42", "title": "feat", "sourceType": "github",
                "metadata": {"number": 42, "draft": false, "mergedAt": null, "closedAt": null}
            }]}
        })
    }

    /// `{data: {b0: …, b1: …}}`
    pub fn aliased(values: Vec<Value>) -> Value {
        let mut data = serde_json::Map::new();
        for (i, v) in values.into_iter().enumerate() {
            data.insert(format!("b{i}"), v);
        }
        serde_json::json!({ "data": data })
    }

    fn one(http: &FakeHttp) -> FetchResult<LinearInfo> {
        fetch_linear_result(http, "http://x", "key", "feature/x")
    }

    fn att(meta: Value) -> LinearPrAttachment {
        LinearPrAttachment { url: "https://github.com/o/r/pull/7".into(), title: "PR".into(), metadata: meta }
    }

    // --- linearAttachmentMatchesBranch / ToPrInfo ---

    #[test]
    fn attachment_branch_matching() {
        assert!(attachment_matches_branch(&att(serde_json::json!({"branch": "feat/a"})), "feat/a"));
        assert!(!attachment_matches_branch(&att(serde_json::json!({"branch": "feat/b"})), "feat/a"));
        assert!(attachment_matches_branch(&att(serde_json::json!({})), "feat/a"));
        assert!(!attachment_matches_branch(&att(serde_json::json!({"branch": "feat"})), "feat/a"));
    }

    #[test]
    fn attachment_to_pr_states() {
        let open = attachment_to_pr_info(&att(serde_json::json!({"number": 7, "draft": true})));
        assert_eq!((open.number, open.state.as_str(), open.is_draft), (7, "OPEN", true));
        assert_eq!(open.checks_status, ChecksStatus::None);
        let merged = attachment_to_pr_info(&att(serde_json::json!({"mergedAt": "2026-01-01", "closedAt": "2026-01-01"})));
        assert_eq!(merged.state, "MERGED");
        let closed = attachment_to_pr_info(&att(serde_json::json!({"mergedAt": null, "closedAt": "2026-01-01"})));
        assert_eq!(closed.state, "CLOSED");
    }

    // --- fetchLinearResult ---

    #[test]
    fn success_maps_ticket_project_and_attachment() {
        let http = FakeHttp::new(vec![ok(aliased(vec![issue("ENG-1")]))]);
        let FetchResult::Ok(Some(info)) = one(&http) else { panic!() };
        assert_eq!(info.identifier, "ENG-1");
        assert_eq!(info.state.kind, "started");
        assert_eq!(info.priority_label, "High");
        assert_eq!(info.assignee, None);
        assert_eq!(info.project.as_ref().unwrap().name, "Dashboard Revamp");
        let a = info.pr_attachment.unwrap();
        assert_eq!(a.url, "https://github.com/test/repo/pull/42");
        assert_eq!(a.metadata["number"], 42);
        // the query is sent with the branch as a variable
        let body: Value = serde_json::from_str(&http.bodies.lock().unwrap()[0]).unwrap();
        assert_eq!(body["variables"]["b0"], "feature/x");
        assert!(body["query"].as_str().unwrap().contains("b0: issueVcsBranchSearch(branchName: $b0)"));
    }

    #[test]
    fn project_is_none_when_the_issue_has_none() {
        let mut i = issue("ENG-1");
        i["project"] = Value::Null;
        i["attachments"] = serde_json::json!({"nodes": []});
        let http = FakeHttp::new(vec![ok(aliased(vec![i]))]);
        let FetchResult::Ok(Some(info)) = one(&http) else { panic!() };
        assert!(info.project.is_none());
        assert!(info.pr_attachment.is_none());
    }

    #[test]
    fn null_issue_is_absence() {
        let http = FakeHttp::new(vec![ok(aliased(vec![Value::Null]))]);
        assert!(matches!(one(&http), FetchResult::Ok(None)));
    }

    #[test]
    fn non_2xx_is_failure() {
        for code in [500, 401] {
            let http = FakeHttp::new(vec![status(code, serde_json::json!({"errors": [{"message": "nope"}]}))]);
            let FetchResult::Err(e) = one(&http) else { panic!("{code}") };
            assert!(e.contains(&code.to_string()), "{e}");
        }
    }

    #[test]
    fn graphql_errors_in_200_body_are_failure() {
        let http = FakeHttp::new(vec![ok(serde_json::json!({"data": null, "errors": [{"message": "Rate limited"}]}))]);
        assert_eq!(one(&http), FetchResult::Err("Rate limited".into()));
    }

    #[test]
    fn unparseable_body_is_failure() {
        let http = FakeHttp::new(vec![Ok(HttpResponse { status: 200, body: "<html>".into() })]);
        assert!(matches!(one(&http), FetchResult::Err(_)));
    }

    #[test]
    fn unreachable_host_is_failure() {
        let http = FakeHttp::new(vec![offline()]);
        let batch = fetch_linear_batch(&http, "http://x", "k", &["a".into()]);
        assert!(batch.network_error.is_some());
        assert!(matches!(batch.results[0].1, FetchResult::Err(_)));
    }

    // --- batching ---

    #[test]
    fn batches_distinct_branches_in_one_request() {
        let http = FakeHttp::new(vec![ok(aliased(vec![issue("ENG-1"), Value::Null]))]);
        let batch = fetch_linear_batch(&http, "http://x", "k", &["a".into(), "b".into(), "a".into()]);
        assert_eq!(http.request_count(), 1);
        assert_eq!(batch.results.len(), 2);
        assert!(matches!(&batch.results[0].1, FetchResult::Ok(Some(i)) if i.identifier == "ENG-1"));
        assert!(matches!(batch.results[1].1, FetchResult::Ok(None)));
    }

    #[test]
    fn alias_scoped_error_fails_only_that_branch() {
        let body = serde_json::json!({
            "data": {"b0": issue("ENG-1"), "b1": null},
            "errors": [{"message": "boom", "path": ["b1"]}]
        });
        let http = FakeHttp::new(vec![ok(body)]);
        let batch = fetch_linear_batch(&http, "http://x", "k", &["a".into(), "b".into()]);
        assert!(matches!(batch.results[0].1, FetchResult::Ok(Some(_))));
        assert_eq!(batch.results[1].1, FetchResult::Err("boom".into()));
    }

    #[test]
    fn network_failure_aborts_remaining_chunks() {
        let names: Vec<String> = (0..LINEAR_CHUNK * 2 + 1).map(|i| format!("br{i}")).collect();
        let http = FakeHttp::new(vec![offline()]);
        let batch = fetch_linear_batch(&http, "http://x", "k", &names);
        assert_eq!(http.request_count(), 1, "no request after the network failed");
        assert_eq!(batch.results.len(), names.len());
        assert!(batch.results.iter().all(|(_, r)| matches!(r, FetchResult::Err(_))));
    }

    #[test]
    fn server_errors_count_as_unavailable() {
        let http = FakeHttp::new(vec![status(503, serde_json::json!({"message": "simulated outage"}))]);
        let batch = fetch_linear_batch(&http, "http://x", "k", &["a".into()]);
        assert!(batch.network_error.is_some());
        let http = FakeHttp::new(vec![status(401, serde_json::json!({}))]);
        let batch = fetch_linear_batch(&http, "http://x", "k", &["a".into()]);
        assert!(batch.network_error.is_none());
    }

    // --- verifyLinearApiKey ---

    #[test]
    fn verify_key() {
        let http = FakeHttp::new(vec![ok(serde_json::json!({"data": {"viewer": {"name": "Tom", "email": "e"}}}))]);
        assert_eq!(verify_api_key(&http, "http://x", "k"), Ok("Tom".into()));
        let http = FakeHttp::new(vec![status(
            401,
            serde_json::json!({"errors": [{"message": "Authentication required, not authenticated"}]}),
        )]);
        assert_eq!(verify_api_key(&http, "http://x", "k"), Err("Authentication required, not authenticated".into()));
        let http = FakeHttp::new(vec![Ok(HttpResponse { status: 401, body: "nope".into() })]);
        assert_eq!(verify_api_key(&http, "http://x", "k"), Err("HTTP 401".into()));
    }

    #[test]
    fn status_colors() {
        assert_eq!(linear_status_color("started"), "cyan");
        assert_eq!(linear_status_color("completed"), "green");
        assert_eq!(linear_status_color("canceled"), "red");
        assert_eq!(linear_status_color("backlog"), "gray");
    }

    // --- real transport against a local socket ---

    fn serve_once(response: &'static str) -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = listener.accept() {
                let mut buf = [0u8; 8192];
                let _ = s.read(&mut buf);
                let _ = s.write_all(response.as_bytes());
            }
        });
        format!("http://{addr}/graphql")
    }

    #[test]
    fn ureq_transport_returns_non_2xx_as_a_response() {
        let url = serve_once("HTTP/1.1 500 Internal Server Error\r\nContent-Length: 4\r\nConnection: close\r\n\r\noops");
        let r = UreqTransport::new(Duration::from_secs(5)).post_json(&url, &[("Authorization", "k")], "{}").unwrap();
        assert_eq!(r, HttpResponse { status: 500, body: "oops".into() });
    }

    #[test]
    fn ureq_transport_classifies_connection_refused_as_network() {
        let port = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        let err = UreqTransport::new(Duration::from_secs(5))
            .post_json(&format!("http://127.0.0.1:{port}/graphql"), &[], "{}")
            .unwrap_err();
        assert!(err.network, "{err:?}");
    }
}
