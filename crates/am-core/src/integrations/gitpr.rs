//! PR facts derived from git alone: no API quota, no rate limit.
//!
//! GitHub publishes every pull request on the git protocol as
//! `refs/pull/<N>/head` (it follows pushes while the PR is open), and its merge
//! strategies leave fingerprints on the base branch. So one
//! `git ls-remote origin` per repo tells which worktree branches have a PR and
//! its number, and local history tells whether it merged:
//!
//! - **PR number**: a branch whose tip (on origin, or local) equals some
//!   `refs/pull/N/head` has PR #N (highest N wins; tips equal to the default
//!   branch's are ignored, a branch with no commits of its own has no PR).
//! - **Merged**: `#N` in the base branch's history, as a squash subject
//!   `… (#N)` or a `Merge pull request #N from …` commit, or the PR head being
//!   an ancestor of the base (GitHub marks such a PR merged itself). Rebase
//!   merges leave neither; the REST detector and the sweep catch those.
//! - **Conflicts**: `git merge-tree --write-tree origin/<base> <head>` exits 1.
//!
//! Draft, review, checks and closed-without-merging aren't in git; the API
//! still answers those, but only for branches these facts (or the detector)
//! say changed. `refs/pull/N/merge` is no open/closed signal: GitHub keeps it
//! after a PR closes and leaves it stale when the PR conflicts.
//!
//! A remote with no `refs/pull/*` at all (not GitHub, a local bare origin, a
//! GitHub repo that never had a PR) yields no facts, and the caller falls back
//! to plain polling.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::{Duration, Instant};

use super::breaker::{is_network_error, summarize_error};
use super::github::{GhOutput, run_program};
use crate::debug;

pub const LS_REMOTE_TIMEOUT: Duration = Duration::from_secs(20);
pub const FETCH_TIMEOUT: Duration = Duration::from_secs(60);
const LOCAL_TIMEOUT: Duration = Duration::from_secs(10);
/// Base-branch subjects searched for `(#N)`.
const SUBJECTS: &str = "--max-count=5000";

/// Runs `git <args>` in `cwd`. Swapped for a fixture runner in tests.
pub trait GitRunner: Send {
    fn run(&self, cwd: &Path, args: &[String], timeout: Duration) -> GhOutput;
}

/// The real `git` on `$PATH`.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemGit;

impl GitRunner for SystemGit {
    fn run(&self, cwd: &Path, args: &[String], timeout: Duration) -> GhOutput {
        // Never block on a credential prompt: stdin is closed anyway.
        run_program("git", cwd, args, timeout, &[("GIT_TERMINAL_PROMPT", "0")])
    }
}

fn args(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

// ---------------------------------------------------------------------------
// Pure parsing
// ---------------------------------------------------------------------------

/// What `git ls-remote --symref origin HEAD refs/heads/* refs/pull/*/head` saw.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RemoteRefs {
    /// From `ref: refs/heads/<x>\tHEAD`.
    pub default_branch: Option<String>,
    /// Branch → tip SHA.
    pub heads: HashMap<String, String>,
    /// PR head SHA → highest PR number pointing at it.
    pub pulls: HashMap<String, u64>,
    /// How many `refs/pull/*/head` refs exist (0 = no PR facts possible).
    pub pull_refs: usize,
}

pub fn parse_ls_remote(out: &str) -> RemoteRefs {
    let mut refs = RemoteRefs::default();
    for line in out.lines() {
        let Some((left, name)) = line.split_once('\t') else { continue };
        if let Some(target) = left.strip_prefix("ref: ") {
            if name == "HEAD" {
                refs.default_branch = target.strip_prefix("refs/heads/").map(str::to_string);
            }
            continue;
        }
        let sha = left.trim();
        if let Some(branch) = name.strip_prefix("refs/heads/") {
            refs.heads.insert(branch.to_string(), sha.to_string());
        } else if let Some(n) =
            name.strip_prefix("refs/pull/").and_then(|r| r.strip_suffix("/head")).and_then(|n| n.parse::<u64>().ok())
        {
            refs.pull_refs += 1;
            let e = refs.pulls.entry(sha.to_string()).or_insert(n);
            *e = (*e).max(n);
        }
    }
    refs
}

/// PR numbers named by base-branch subjects: GitHub's squash subject
/// `Title (#12)` and merge commit `Merge pull request #12 from owner/branch`.
pub fn merged_numbers(subjects: &str) -> HashSet<u64> {
    let mut out = HashSet::new();
    for s in subjects.lines().map(str::trim) {
        if let Some(rest) = s.strip_prefix("Merge pull request #") {
            if let Some(n) = rest.split_whitespace().next().and_then(|n| n.parse().ok()) {
                out.insert(n);
            }
        } else if let Some(open) = s.strip_suffix(')').and_then(|s| s.rfind("(#").map(|i| &s[i + 2..]))
            && let Ok(n) = open.parse()
        {
            out.insert(n);
        }
    }
    out
}

/// `https://<host>/<owner>/<repo>` for a remote URL (`git@host:o/r.git`,
/// `ssh://git@host/o/r`, `https://host/o/r.git`).
pub fn web_base(remote_url: &str) -> Option<String> {
    let url = remote_url.trim().trim_end_matches('/').trim_end_matches(".git");
    let (host, path) = if let Some(rest) = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://")) {
        let rest = rest.rsplit_once('@').map_or(rest, |(_, r)| r);
        rest.split_once('/')?
    } else if let Some(rest) = url.strip_prefix("ssh://") {
        let rest = rest.rsplit_once('@').map_or(rest, |(_, r)| r);
        let (host, path) = rest.split_once('/')?;
        (host.split(':').next()?, path)
    } else {
        let rest = url.rsplit_once('@').map_or(url, |(_, r)| r);
        rest.split_once(':')?
    };
    (!host.is_empty() && path.split('/').filter(|p| !p.is_empty()).count() == 2).then(|| format!("https://{host}/{path}"))
}

// ---------------------------------------------------------------------------
// Per-repo scanner
// ---------------------------------------------------------------------------

/// What git says about one branch's PR.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GitPr {
    pub number: u64,
    pub merged: bool,
    /// `None` when it couldn't be computed (base or head not available locally).
    pub conflicts: Option<bool>,
}

/// What one [`RepoGit::scan`] changed.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ScanResult {
    /// Branches the API should look at now: a PR appeared or changed number,
    /// it merged, or its head moved.
    pub dirty: Vec<String>,
    /// Any fact changed (including conflicts), so the UI needs an update.
    pub changed: bool,
}

#[derive(Debug, Clone)]
struct Derived {
    head: String,
    base_sha: Option<String>,
    pr: GitPr,
}

/// Git-derived PR state for one repo, refreshed by [`RepoGit::scan`].
#[derive(Debug, Default)]
pub struct RepoGit {
    remote: Option<RemoteRefs>,
    /// `refs/heads/*` and `refs/remotes/origin/*` → SHA.
    local: HashMap<String, String>,
    web_base: Option<Option<String>>,
    /// Base ref → (its SHA, PR numbers in its history).
    subjects: HashMap<String, (String, HashSet<u64>)>,
    derived: HashMap<String, Derived>,
    pub last_scan: Option<Instant>,
    pub last_local: Option<Instant>,
}

impl RepoGit {
    /// The remote publishes PR refs, so facts (and their absence) mean something.
    pub fn usable(&self) -> bool {
        self.remote.as_ref().is_some_and(|r| r.pull_refs > 0)
    }

    /// How long to wait between scans: every `base`, but a remote with a huge
    /// PR history (a big monorepo lists megabytes of refs) only every 5 min;
    /// pushes and hints still trigger a scan at once.
    pub fn scan_every(&self, base: Duration) -> Duration {
        match self.remote.as_ref().map_or(0, |r| r.pull_refs) {
            n if n > 20_000 => base.max(Duration::from_secs(300)),
            _ => base,
        }
    }

    pub fn fact(&self, branch: &str) -> Option<GitPr> {
        self.derived.get(branch).map(|d| d.pr)
    }

    /// `https://<host>/<owner>/<repo>/pull/<n>`, when the remote URL parses.
    pub fn pr_url(&self, number: u64) -> Option<String> {
        self.web_base.clone().flatten().map(|b| format!("{b}/pull/{number}"))
    }

    fn read_local(&mut self, git: &dyn GitRunner, repo: &Path) -> bool {
        let out = git.run(
            repo,
            &args(&["for-each-ref", "--format=%(objectname) %(refname)", "refs/heads", "refs/remotes/origin"]),
            LOCAL_TIMEOUT,
        );
        if !out.ok {
            debug!("gitpr", "for-each-ref failed in {}: {}", repo.display(), summarize_error(&out.stderr));
            return false;
        }
        self.local = out
            .stdout
            .lines()
            .filter_map(|l| l.split_once(' ').map(|(sha, name)| (name.to_string(), sha.to_string())))
            .collect();
        true
    }

    /// Re-read local refs (cheap, no network). Returns the tracked branches
    /// whose `refs/remotes/origin/<branch>` moved: someone pushed from here.
    pub fn check_local(&mut self, git: &dyn GitRunner, repo: &Path, branches: &[String], now: Instant) -> Vec<String> {
        self.last_local = Some(now);
        let before: Vec<Option<String>> = branches.iter().map(|b| self.local.get(&remote_ref(b)).cloned()).collect();
        let first = self.local.is_empty();
        if !self.read_local(git, repo) || first {
            return Vec::new();
        }
        branches
            .iter()
            .zip(before)
            .filter(|(b, old)| self.local.get(&remote_ref(b)).is_some_and(|new| old.as_ref() != Some(new)))
            .map(|(b, _)| b.clone())
            .collect()
    }

    /// One `git ls-remote` plus local derivation for every tracked branch.
    /// `bases` maps a branch to its PR's base when the API told us (stacks);
    /// otherwise the remote's default branch is the base.
    /// `Err((message, is_network))` when the remote couldn't be listed.
    pub fn scan(
        &mut self,
        git: &dyn GitRunner,
        repo: &Path,
        branches: &[String],
        bases: &HashMap<String, String>,
        now: Instant,
    ) -> Result<ScanResult, (String, bool)> {
        self.last_scan = Some(now);
        let started = Instant::now();
        let out = git.run(
            repo,
            &args(&["ls-remote", "--symref", "origin", "HEAD", "refs/heads/*", "refs/pull/*/head"]),
            LS_REMOTE_TIMEOUT,
        );
        if !out.ok {
            return Err((summarize_error(&out.stderr), is_network_error(&out.stderr)));
        }
        let refs = parse_ls_remote(&out.stdout);
        let mut result = ScanResult::default();
        if refs.pull_refs == 0 {
            result.changed = !self.derived.is_empty();
            self.derived.clear();
            self.remote = Some(refs);
            return Ok(result);
        }
        if self.web_base.is_none() {
            let url = git.run(repo, &args(&["remote", "get-url", "origin"]), LOCAL_TIMEOUT);
            self.web_base = Some(url.ok.then(|| web_base(&url.stdout)).flatten());
        }
        self.read_local(git, repo);
        let default = refs.default_branch.clone().unwrap_or_else(|| "main".into());
        let default_tip = refs.heads.get(&default).cloned();

        let mut fetched: HashSet<String> = HashSet::new();
        for branch in branches {
            if *branch == default {
                continue;
            }
            let candidates = [refs.heads.get(branch), self.local.get(&format!("refs/heads/{branch}"))];
            let found = candidates
                .into_iter()
                .flatten()
                .filter(|sha| Some(*sha) != default_tip.as_ref())
                .filter_map(|sha| refs.pulls.get(sha).map(|n| (*n, sha.clone())))
                .max_by_key(|(n, _)| *n);
            let Some((number, head)) = found else {
                if self.derived.remove(branch).is_some() {
                    result.changed = true;
                    result.dirty.push(branch.clone());
                }
                continue;
            };

            let base = bases.get(branch).cloned().unwrap_or_else(|| default.clone());
            let base_ref = remote_ref(&base);
            // Keep origin/<base> current so its history shows the merge.
            if let Some(remote_tip) = refs.heads.get(&base)
                && self.local.get(&base_ref) != Some(remote_tip)
                && fetched.insert(base.clone())
            {
                let spec = format!("+refs/heads/{base}:{base_ref}");
                let f = git.run(repo, &args(&["fetch", "--quiet", "--no-tags", "origin", &spec]), FETCH_TIMEOUT);
                if f.ok {
                    self.local.insert(base_ref.clone(), remote_tip.clone());
                } else {
                    debug!("gitpr", "fetch of {base} failed in {}: {}", repo.display(), summarize_error(&f.stderr));
                }
            }
            let base_sha = self.local.get(&base_ref).cloned();

            let prev = self.derived.get(branch).cloned();
            let pr = match &prev {
                Some(d) if d.head == head && d.base_sha == base_sha && d.pr.number == number => d.pr,
                _ => self.derive(git, repo, number, &head, &base_ref, base_sha.as_deref()),
            };
            match prev {
                None => {
                    result.changed = true;
                    result.dirty.push(branch.clone());
                }
                Some(d) => {
                    if d.pr != pr {
                        result.changed = true;
                    }
                    if d.pr.number != number || d.pr.merged != pr.merged || d.head != head {
                        result.dirty.push(branch.clone());
                    }
                }
            }
            self.derived.insert(branch.clone(), Derived { head, base_sha, pr });
        }
        let tracked: HashSet<&String> = branches.iter().collect();
        self.derived.retain(|b, _| tracked.contains(b));
        debug!(
            "gitpr",
            "scan {}: {} PR ref(s), {} tracked PR(s), dirty {:?} in {:?}",
            repo.display(),
            refs.pull_refs,
            self.derived.len(),
            result.dirty,
            started.elapsed()
        );
        self.remote = Some(refs);
        Ok(result)
    }

    fn derive(&mut self, git: &dyn GitRunner, repo: &Path, number: u64, head: &str, base_ref: &str, base_sha: Option<&str>) -> GitPr {
        let Some(base_sha) = base_sha else { return GitPr { number, merged: false, conflicts: None } };
        let stale = self.subjects.get(base_ref).is_none_or(|(sha, _)| sha != base_sha);
        if stale {
            let log = git.run(repo, &args(&["log", "--format=%s", SUBJECTS, base_ref]), LOCAL_TIMEOUT);
            let numbers = if log.ok { merged_numbers(&log.stdout) } else { HashSet::new() };
            self.subjects.insert(base_ref.to_string(), (base_sha.to_string(), numbers));
        }
        let in_subjects = self.subjects.get(base_ref).is_some_and(|(_, ns)| ns.contains(&number));
        let merged = in_subjects || git.run(repo, &args(&["merge-base", "--is-ancestor", head, base_ref]), LOCAL_TIMEOUT).ok;
        let conflicts = if merged {
            None
        } else {
            let mt = git.run(
                repo,
                &args(&["merge-tree", "--write-tree", "--name-only", "--no-messages", base_ref, head]),
                LOCAL_TIMEOUT,
            );
            // 0 = clean, 1 = conflicts; anything else (old git, missing object) = unknown.
            match (mt.ok, mt.stdout.is_empty()) {
                (true, _) => Some(false),
                (false, false) if mt.stderr.trim().is_empty() => Some(true),
                _ => None,
            }
        };
        GitPr { number, merged, conflicts }
    }
}

fn remote_ref(branch: &str) -> String {
    format!("refs/remotes/origin/{branch}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn parses_ls_remote() {
        let out = "ref: refs/heads/main\tHEAD\n\
                   aaa\tHEAD\n\
                   aaa\trefs/heads/main\n\
                   bbb\trefs/heads/feat/x\n\
                   bbb\trefs/pull/3/head\n\
                   bbb\trefs/pull/7/head\n\
                   ccc\trefs/pull/12/head\n\
                   ddd\trefs/pull/12/merge\n";
        let r = parse_ls_remote(out);
        assert_eq!(r.default_branch.as_deref(), Some("main"));
        assert_eq!(r.heads.get("feat/x").map(String::as_str), Some("bbb"));
        assert_eq!(r.pulls.get("bbb"), Some(&7), "highest number wins");
        assert_eq!(r.pulls.get("ccc"), Some(&12));
        assert_eq!(r.pull_refs, 3, "merge refs aren't PR heads");
    }

    #[test]
    fn finds_merged_pr_numbers_in_subjects() {
        let s = "chore: release v2.1.36\n\
                 feat: blank line above a cluster (#168)\n\
                 Merge pull request #12 from owner/feature/x\n\
                 Revert \"feat: thing (#9)\"\n\
                 fix: mention (#abc)\n";
        assert_eq!(merged_numbers(s), HashSet::from([168, 12]));
    }

    #[test]
    fn web_base_from_remote_urls() {
        assert_eq!(web_base("git@github.com:o/r.git").as_deref(), Some("https://github.com/o/r"));
        assert_eq!(web_base("https://github.com/o/r.git\n").as_deref(), Some("https://github.com/o/r"));
        assert_eq!(web_base("https://x:tok@github.com/o/r").as_deref(), Some("https://github.com/o/r"));
        assert_eq!(web_base("ssh://git@ghe.corp:22/o/r.git").as_deref(), Some("https://ghe.corp/o/r"));
        assert_eq!(web_base("/srv/git/origin.git"), None);
    }

    // --- real repositories: a bare "GitHub" origin with refs/pull/N/head ---

    fn sh(dir: &Path, a: &[&str]) -> String {
        let out = Command::new("git").arg("-C").arg(dir).args(a).output().unwrap();
        assert!(out.status.success(), "git {a:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        origin: std::path::PathBuf,
        work: std::path::PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let origin = dir.path().join("origin.git");
            let work = dir.path().join("work");
            sh(dir.path(), &["init", "-q", "--bare", "-b", "main", origin.to_str().unwrap()]);
            sh(dir.path(), &["clone", "-q", origin.to_str().unwrap(), work.to_str().unwrap()]);
            for (k, v) in [("user.email", "t@t"), ("user.name", "t"), ("commit.gpgsign", "false")] {
                sh(&work, &["config", k, v]);
            }
            let f = Fixture { _dir: dir, origin, work };
            f.commit("README", "hello\n", "init");
            sh(&f.work, &["push", "-q", "origin", "main"]);
            f
        }
        fn commit(&self, file: &str, body: &str, msg: &str) -> String {
            std::fs::write(self.work.join(file), body).unwrap();
            sh(&self.work, &["add", file]);
            sh(&self.work, &["commit", "-q", "-m", msg]);
            sh(&self.work, &["rev-parse", "HEAD"])
        }
        /// A branch with one commit, pushed, with PR #n pointing at its tip.
        fn pr_branch(&self, branch: &str, n: u64, file: &str, body: &str) -> String {
            sh(&self.work, &["checkout", "-q", "-b", branch, "main"]);
            let tip = self.commit(file, body, &format!("work on {branch}"));
            sh(&self.work, &["push", "-q", "origin", branch]);
            sh(&self.origin, &["update-ref", &format!("refs/pull/{n}/head"), &tip]);
            sh(&self.work, &["checkout", "-q", "main"]);
            tip
        }
        /// Squash-merge `branch` into main on "GitHub" (the origin), the way
        /// GitHub does: a new commit whose subject ends in `(#n)`.
        fn squash_merge_on_origin(&self, branch: &str, n: u64) {
            sh(&self.work, &["checkout", "-q", "main"]);
            sh(&self.work, &["merge", "-q", "--squash", branch]);
            sh(&self.work, &["commit", "-q", "-m", &format!("feat: {branch} (#{n})")]);
            sh(&self.work, &["push", "-q", "origin", "main"]);
            // The clone's own origin/main now knows; rewind it so the scan must fetch.
            sh(&self.work, &["update-ref", "refs/remotes/origin/main", "HEAD~1"]);
            sh(&self.work, &["reset", "-q", "--hard", "HEAD~1"]);
        }
        fn scan(&self, g: &mut RepoGit, branches: &[&str]) -> ScanResult {
            let bs: Vec<String> = branches.iter().map(|s| s.to_string()).collect();
            g.scan(&SystemGit, &self.work, &bs, &HashMap::new(), Instant::now()).unwrap()
        }
    }

    #[test]
    fn no_pull_refs_means_no_facts() {
        let f = Fixture::new();
        sh(&f.work, &["checkout", "-q", "-b", "feat/a"]);
        f.commit("a", "a\n", "a");
        sh(&f.work, &["push", "-q", "origin", "feat/a"]);
        let mut g = RepoGit::default();
        let r = f.scan(&mut g, &["feat/a"]);
        assert!(!g.usable());
        assert_eq!(r, ScanResult::default());
        assert_eq!(g.fact("feat/a"), None);
    }

    #[test]
    fn finds_open_pr_then_sees_it_squash_merged() {
        let f = Fixture::new();
        f.pr_branch("feat/a", 5, "a", "a\n");
        f.pr_branch("feat/nopr", 0, "n", "n\n");
        sh(&f.origin, &["update-ref", "-d", "refs/pull/0/head"]);
        let mut g = RepoGit::default();
        let r = f.scan(&mut g, &["main", "feat/a", "feat/nopr"]);
        assert!(g.usable());
        assert_eq!(r.dirty, vec!["feat/a"]);
        assert_eq!(g.fact("feat/a"), Some(GitPr { number: 5, merged: false, conflicts: Some(false) }));
        assert_eq!(g.fact("feat/nopr"), None);
        assert_eq!(g.fact("main"), None);

        // Nothing moved: nothing to do.
        assert_eq!(f.scan(&mut g, &["main", "feat/a", "feat/nopr"]), ScanResult::default());

        f.squash_merge_on_origin("feat/a", 5);
        let r = f.scan(&mut g, &["main", "feat/a", "feat/nopr"]);
        assert_eq!(r.dirty, vec!["feat/a"], "fetched origin/main and found (#5)");
        assert!(g.fact("feat/a").unwrap().merged);
    }

    #[test]
    fn merge_commit_counts_as_merged() {
        let f = Fixture::new();
        f.pr_branch("feat/m", 9, "m", "m\n");
        sh(&f.work, &["merge", "-q", "--no-ff", "feat/m", "-m", "Merge pull request #9 from o/feat/m"]);
        sh(&f.work, &["push", "-q", "origin", "main"]);
        let mut g = RepoGit::default();
        f.scan(&mut g, &["feat/m"]);
        assert_eq!(g.fact("feat/m"), Some(GitPr { number: 9, merged: true, conflicts: None }));
    }

    #[test]
    fn conflicting_pr_is_flagged() {
        let f = Fixture::new();
        f.pr_branch("feat/c", 4, "README", "branch side\n");
        f.commit("README", "main side\n", "main edit");
        sh(&f.work, &["push", "-q", "origin", "main"]);
        let mut g = RepoGit::default();
        f.scan(&mut g, &["feat/c"]);
        assert_eq!(g.fact("feat/c"), Some(GitPr { number: 4, merged: false, conflicts: Some(true) }));
    }

    #[test]
    fn a_branch_at_the_default_tip_has_no_pr() {
        let f = Fixture::new();
        let tip = sh(&f.work, &["rev-parse", "main"]);
        sh(&f.work, &["branch", "feat/empty", "main"]);
        // Even if some old PR head happens to equal main's tip.
        sh(&f.origin, &["update-ref", "refs/pull/1/head", &tip]);
        let mut g = RepoGit::default();
        f.scan(&mut g, &["feat/empty"]);
        assert_eq!(g.fact("feat/empty"), None);
    }

    #[test]
    fn pushing_moves_the_remote_ref_and_the_pr_head() {
        let f = Fixture::new();
        f.pr_branch("feat/p", 2, "p", "p\n");
        let mut g = RepoGit::default();
        let bs = vec!["feat/p".to_string()];
        assert!(g.check_local(&SystemGit, &f.work, &bs, Instant::now()).is_empty(), "first read is a baseline");
        f.scan(&mut g, &["feat/p"]);

        sh(&f.work, &["checkout", "-q", "feat/p"]);
        let tip = f.commit("p", "p2\n", "more");
        sh(&f.work, &["push", "-q", "origin", "feat/p"]);
        sh(&f.origin, &["update-ref", "refs/pull/2/head", &tip]);
        assert_eq!(g.check_local(&SystemGit, &f.work, &bs, Instant::now()), bs);
        assert_eq!(f.scan(&mut g, &["feat/p"]).dirty, bs, "head moved");
    }

    #[test]
    fn a_local_path_origin_has_no_pr_url() {
        let f = Fixture::new();
        f.pr_branch("feat/u", 3, "u", "u\n");
        let mut g = RepoGit::default();
        f.scan(&mut g, &["feat/u"]);
        // A local-path origin has no web URL.
        assert_eq!(g.pr_url(3), None);
    }
}
