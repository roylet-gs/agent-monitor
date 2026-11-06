//! Conditional REST GETs through `gh api -i`.
//!
//! GraphQL has no conditional requests, but REST does: a `304 Not Modified`
//! answer to an `If-None-Match` / `If-Modified-Since` request costs nothing
//! against the primary rate limit (verified: `X-RateLimit-Used` doesn't move).
//! The live watchers ([`super::notifications`], [`super::ci_watch`]) poll with
//! these to learn *that* something changed, and only then spend a GraphQL
//! fetch on it.
//!
//! `gh api -i` prints the status line and headers (CRLF), a blank line, then
//! the body. It exits 1 on a 304 (`gh: HTTP 304` on stderr) although the
//! request succeeded, so the status line on stdout decides, never the exit code.

use std::path::Path;
use std::time::Duration;

use super::github::GhRunner;

pub const REST_TIMEOUT: Duration = Duration::from_secs(10);

/// One parsed `gh api -i` response.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RestResponse {
    pub status: u16,
    /// Header names lower-cased.
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl RestResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    pub fn not_modified(&self) -> bool {
        self.status == 304
    }

    /// `X-RateLimit-Remaining` of the bucket this request counted against.
    pub fn rate_remaining(&self) -> Option<u64> {
        self.header("x-ratelimit-remaining").and_then(|v| v.parse().ok())
    }

    /// `X-Poll-Interval`, the minimum seconds GitHub asks between polls.
    pub fn poll_interval(&self) -> Option<Duration> {
        self.header("x-poll-interval").and_then(|v| v.parse().ok()).map(Duration::from_secs)
    }
}

/// Parse `gh api -i` stdout. `None` when it doesn't start with a status line.
pub fn parse_include(stdout: &str) -> Option<RestResponse> {
    let mut rest = stdout;
    let mut next_line = || {
        let (line, tail) = rest.split_once('\n').unwrap_or((rest, ""));
        rest = tail;
        line.trim_end_matches('\r')
    };
    let status_line = next_line();
    if !status_line.starts_with("HTTP/") {
        return None;
    }
    let status: u16 = status_line.split_whitespace().nth(1)?.parse().ok()?;
    let mut headers = Vec::new();
    loop {
        let line = next_line();
        if line.is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
    }
    Some(RestResponse { status, headers, body: rest.to_string() })
}

/// `gh api -i <path>` with the given request headers, in `cwd` (so
/// `{owner}/{repo}` in `path` resolve from that checkout's remote).
///
/// `Ok` for any HTTP answer, 304 included; `Err` when there was no answer
/// (gh missing, a network failure, a timeout).
pub fn rest_get(gh: &dyn GhRunner, cwd: &Path, path: &str, headers: &[(&str, &str)]) -> Result<RestResponse, String> {
    let mut args: Vec<String> = vec!["api".into(), "-i".into()];
    for (k, v) in headers {
        args.push("-H".into());
        args.push(format!("{k}: {v}"));
    }
    args.push(path.to_string());
    let out = gh.run(cwd, &args, REST_TIMEOUT);
    match parse_include(&out.stdout) {
        Some(r) => Ok(r),
        None if out.stderr.trim().is_empty() => Err(format!("no HTTP response from gh api {path}")),
        None => Err(out.stderr.trim().to_string()),
    }
}

/// Percent-encode one path segment (a branch name: `feature/x` → `feature%2Fx`).
pub fn encode_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A `gh api -i` stdout for tests.
    pub fn include(status: u16, headers: &[(&str, &str)], body: &str) -> String {
        let reason = if status == 304 { "Not Modified" } else { "OK" };
        let mut s = format!("HTTP/2.0 {status} {reason}\r\n");
        for (k, v) in headers {
            s.push_str(&format!("{k}: {v}\r\n"));
        }
        s.push_str("\r\n");
        s.push_str(body);
        s
    }

    #[test]
    fn parses_status_headers_and_body() {
        let r = parse_include(&include(200, &[("ETag", "W/\"abc\""), ("X-Poll-Interval", "60")], "[1]")).unwrap();
        assert_eq!(r.status, 200);
        assert_eq!(r.header("etag"), Some("W/\"abc\""));
        assert_eq!(r.poll_interval(), Some(Duration::from_secs(60)));
        assert_eq!(r.body, "[1]");
    }

    #[test]
    fn a_304_has_no_body() {
        let r = parse_include(&include(304, &[("X-RateLimit-Remaining", "4862")], "")).unwrap();
        assert!(r.not_modified());
        assert_eq!(r.rate_remaining(), Some(4862));
        assert_eq!(r.body, "");
    }

    #[test]
    fn a_304_counts_although_gh_exits_1() {
        use super::super::github::GhOutput;
        use super::super::github::tests::FakeGh;
        let gh = FakeGh::new(vec![GhOutput {
            ok: false,
            stdout: include(304, &[], ""),
            stderr: "gh: HTTP 304".into(),
        }]);
        let r = rest_get(&gh, Path::new("/"), "notifications", &[("If-Modified-Since", "x")]).unwrap();
        assert!(r.not_modified());
        assert_eq!(gh.calls.lock().unwrap()[0], ["api", "-i", "-H", "If-Modified-Since: x", "notifications"]);
    }

    #[test]
    fn no_status_line_is_an_error() {
        use super::super::github::GhOutput;
        use super::super::github::tests::FakeGh;
        let gh = FakeGh::new(vec![GhOutput::failure("error connecting to api.github.com")]);
        assert_eq!(rest_get(&gh, Path::new("/"), "x", &[]).unwrap_err(), "error connecting to api.github.com");
    }

    #[test]
    fn encodes_branch_segments() {
        assert_eq!(encode_segment("ai/feature/DES-173_x.y"), "ai%2Ffeature%2FDES-173_x.y");
        assert_eq!(encode_segment("a b#c"), "a%20b%23c");
    }
}
