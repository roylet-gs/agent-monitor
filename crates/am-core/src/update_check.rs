//! "Is there a newer `am`?" — compares the compiled-in version with the latest
//! public GitHub Release of [`OWN_REPO`] (tags are `vX.Y.Z`).
//!
//! The result is cached in `update-check.json` so restarts and several open
//! dashboards share one request per [`CHECK_INTERVAL`]. Only a successful
//! fetch writes the cache; being offline is normal and only logged at DEBUG.

use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{debug, info, paths, timefmt};

pub const OWN_REPO: &str = "roylet-gs/agent-monitor";

/// How long a cached answer stays fresh.
pub const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CachedCheck {
    /// Unix seconds of the last successful fetch.
    pub checked_at: i64,
    /// Latest released version, without the leading `v`.
    pub latest: String,
}

/// How this copy of `am` was installed, guessed from the binary's real path,
/// so the update hint can name the right command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallMethod {
    Homebrew,
    Npm,
    Pnpm,
    /// `cargo install --path …` into `~/.cargo/bin`.
    Cargo,
    /// Run straight from a checkout's `target/`.
    DevBuild,
    Unknown,
}

impl InstallMethod {
    /// Detect from the running binary (symlinks resolved, so Homebrew's
    /// `bin/am-tui` → `Cellar/…` and npm's shims are seen for what they are).
    pub fn current() -> InstallMethod {
        let exe = std::env::current_exe().ok().map(|e| e.canonicalize().unwrap_or(e));
        exe.map_or(InstallMethod::Unknown, |e| Self::from_exe(&e))
    }

    pub fn from_exe(exe: &Path) -> InstallMethod {
        let p = exe.to_string_lossy();
        if p.contains("/Cellar/agent-monitor/") {
            InstallMethod::Homebrew
        } else if p.contains("/node_modules/@roylet-gs/agent-monitor") {
            // pnpm's global store keeps packages under `.pnpm/`.
            if p.contains("/.pnpm/") { InstallMethod::Pnpm } else { InstallMethod::Npm }
        } else if p.contains("/.cargo/bin/") {
            InstallMethod::Cargo
        } else if p.contains("/target/debug/") || p.contains("/target/release/") {
            InstallMethod::DevBuild
        } else {
            InstallMethod::Unknown
        }
    }

    /// The command (or instruction) that installs the latest release.
    pub fn upgrade_hint(self) -> String {
        match self {
            InstallMethod::Homebrew => "brew upgrade agent-monitor".into(),
            InstallMethod::Npm => "npm install -g @roylet-gs/agent-monitor@latest".into(),
            InstallMethod::Pnpm => "pnpm add -g @roylet-gs/agent-monitor@latest".into(),
            InstallMethod::Cargo => "in your checkout: git pull, then cargo install --path crates/am-cli, crates/am-tui and crates/am-hook".into(),
            InstallMethod::DevBuild => "git pull, then cargo build".into(),
            InstallMethod::Unknown => format!("see https://github.com/{OWN_REPO}/releases/latest"),
        }
    }
}

/// A newer release, and how to get it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Update {
    pub latest: String,
    pub current: String,
    pub hint: String,
}

impl Update {
    pub fn new(latest: String, method: InstallMethod) -> Update {
        Update { latest, current: env!("CARGO_PKG_VERSION").into(), hint: method.upgrade_hint() }
    }
}

/// `Some(latest)` when a newer release than `current` exists. Uses the cache
/// when it is fresh, otherwise asks GitHub. Blocking; call it off the UI thread.
pub fn check(current: &str) -> Option<String> {
    let path = paths::update_check_path();
    let now = timefmt::now_unix();
    let latest = match fresh_cache(&path, now) {
        Some(c) => {
            debug!("update", "cached latest={} checked_at={}", c.latest, c.checked_at);
            c.latest
        }
        None => match fetch_latest() {
            Ok(latest) => {
                write_cache(&path, &CachedCheck { checked_at: now, latest: latest.clone() });
                latest
            }
            Err(e) => {
                debug!("update", "release check failed: {e}");
                return None;
            }
        },
    };
    if is_newer(&latest, current) {
        info!("update", "newer release available: {latest} (running {current})");
        Some(latest)
    } else {
        debug!("update", "up to date: latest={latest} current={current}");
        None
    }
}

/// The cached check, if it is younger than [`CHECK_INTERVAL`].
pub fn fresh_cache(path: &Path, now: i64) -> Option<CachedCheck> {
    let c: CachedCheck = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let age = now - c.checked_at;
    (0..CHECK_INTERVAL.as_secs() as i64).contains(&age).then_some(c)
}

fn write_cache(path: &Path, c: &CachedCheck) {
    let Ok(json) = serde_json::to_string(c) else { return };
    let tmp = path.with_extension("json.tmp");
    if let Err(e) = std::fs::write(&tmp, json).and_then(|_| std::fs::rename(&tmp, path)) {
        debug!("update", "could not write {}: {e}", path.display());
    }
}

fn fetch_latest() -> Result<String, String> {
    let agent: ureq::Agent =
        ureq::Agent::config_builder().timeout_global(Some(HTTP_TIMEOUT)).http_status_as_error(false).build().into();
    let url = format!("https://api.github.com/repos/{OWN_REPO}/releases/latest");
    let mut resp = agent
        .get(&url)
        .header("User-Agent", concat!("agent-monitor/", env!("CARGO_PKG_VERSION")))
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    let body = resp.body_mut().read_to_string().map_err(|e| e.to_string())?;
    if !(200..300).contains(&status) {
        return Err(format!("HTTP {status}"));
    }
    parse_latest(&body).ok_or_else(|| "no tag_name in response".into())
}

/// `tag_name` from a `releases/latest` body, without the leading `v`.
pub fn parse_latest(body: &str) -> Option<String> {
    let v: Value = serde_json::from_str(body).ok()?;
    let tag = v.get("tag_name")?.as_str()?;
    let version = tag.strip_prefix('v').unwrap_or(tag);
    parse_version(version).map(|_| version.to_string())
}

fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let core = v.split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
    let version = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(version)
}

/// Whether `latest` is a strictly higher `X.Y.Z` than `current`. Anything
/// unparseable is treated as "not newer".
pub fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions_numerically() {
        assert!(is_newer("2.2.0", "2.1.1"));
        assert!(is_newer("2.10.0", "2.9.9"));
        assert!(is_newer("3.0.0", "2.99.99"));
        assert!(!is_newer("2.1.1", "2.1.1"));
        assert!(!is_newer("2.1.0", "2.1.1"));
        assert!(!is_newer("garbage", "2.1.1"));
        assert!(!is_newer("2.2", "2.1.1"));
        assert!(is_newer("2.2.0-beta", "2.1.1"));
    }

    #[test]
    fn detects_install_method_from_the_binary_path() {
        use InstallMethod::*;
        let cases = [
            ("/opt/homebrew/Cellar/agent-monitor/2.1.1/bin/am-tui", Homebrew),
            ("/usr/local/Cellar/agent-monitor/2.1.1/bin/am-tui", Homebrew),
            ("/usr/local/lib/node_modules/@roylet-gs/agent-monitor/node_modules/@roylet-gs/agent-monitor-darwin-arm64/bin/am-tui", Npm),
            ("/Users/me/.nvm/versions/node/v22.1.0/lib/node_modules/@roylet-gs/agent-monitor-linux-x64/bin/am-tui", Npm),
            ("/Users/me/Library/pnpm/global/5/.pnpm/@roylet-gs+agent-monitor-darwin-arm64@2.1.1/node_modules/@roylet-gs/agent-monitor-darwin-arm64/bin/am-tui", Pnpm),
            ("/Users/me/.cargo/bin/am-tui", Cargo),
            ("/Users/me/src/agent-monitor/target/release/am-tui", DevBuild),
            ("/usr/bin/am-tui", Unknown),
        ];
        for (path, want) in cases {
            assert_eq!(InstallMethod::from_exe(Path::new(path)), want, "{path}");
        }
        assert_eq!(Homebrew.upgrade_hint(), "brew upgrade agent-monitor");
        assert!(Unknown.upgrade_hint().contains("releases/latest"));
    }

    #[test]
    fn parses_tag_name() {
        assert_eq!(parse_latest(r#"{"tag_name":"v2.1.1","name":"v2.1.1"}"#).as_deref(), Some("2.1.1"));
        assert_eq!(parse_latest(r#"{"tag_name":"2.3.0"}"#).as_deref(), Some("2.3.0"));
        assert_eq!(parse_latest(r#"{"tag_name":"nightly"}"#), None);
        assert_eq!(parse_latest(r#"{"message":"Not Found"}"#), None);
        assert_eq!(parse_latest("not json"), None);
    }

    #[test]
    fn cache_is_fresh_only_inside_the_interval() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("update-check.json");
        assert_eq!(fresh_cache(&path, 1_000), None);

        write_cache(&path, &CachedCheck { checked_at: 1_000, latest: "2.2.0".into() });
        let hit = fresh_cache(&path, 1_000 + 60).unwrap();
        assert_eq!(hit.latest, "2.2.0");
        let stale = 1_000 + CHECK_INTERVAL.as_secs() as i64;
        assert_eq!(fresh_cache(&path, stale), None);
        // A clock that went backwards doesn't pin a stale answer forever.
        assert_eq!(fresh_cache(&path, 500), None);

        std::fs::write(&path, "garbage").unwrap();
        assert_eq!(fresh_cache(&path, 1_000), None);
    }
}
