//! Worktree lifecycle (no UI): repository registration, syncing the DB with
//! `git worktree list`, branch-existence checks, worktree creation/deletion,
//! the per-repo worktree limit, and per-repo startup scripts.
//!
//! Ported from `src/lib/git.ts`, `src/lib/sync.ts`, `src/lib/worktree-limit.ts`,
//! `src/lib/scripts.ts` and the create/delete flows in
//! `src/commands/worktree/{create,delete}.ts` + `src/app.tsx`
//! (`doCreateWorktree` / `handleDelete`). Both apps share the DB and the
//! on-disk layout, so the worktree location (`<repo>/.claude/worktrees/<branch
//! with / → ->`), DB SQL, and script paths match the TS app exactly.
//!
//! Everything here is blocking and shells out to `git` (and `gh` for the
//! remote-branch probe); run it off the UI thread.

use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rusqlite::Connection;

use crate::db;
use crate::integrations::github::{GhRunner, SystemGh};
use crate::model::{Repository, Worktree};
use crate::settings::Settings;
use crate::{debug, info, warn};

// ---------------------------------------------------------------------------
// Process helpers
// ---------------------------------------------------------------------------

struct CmdOut {
    /// `None` when the process was killed (timeout) or terminated by a signal.
    code: Option<i32>,
    stdout: String,
    stderr: String,
    timed_out: bool,
}

/// Runs `git -C <dir> <args>`, optionally killing it after `timeout`.
/// `Err` only when git could not be spawned at all.
fn run_git(dir: &Path, args: &[&str], timeout: Option<Duration>) -> Result<CmdOut, String> {
    use std::io::Read;
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(dir)
        .args(args)
        // Never block on a credential/SSH prompt: stdin is closed anyway.
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let Some(timeout) = timeout else {
        let out = cmd.output().map_err(|e| format!("failed to run git: {e}"))?;
        return Ok(CmdOut {
            code: out.status.code(),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            timed_out: false,
        });
    };
    let mut child = cmd.spawn().map_err(|e| format!("failed to run git: {e}"))?;
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
    let (code, timed_out) = loop {
        match child.try_wait() {
            Ok(Some(status)) => break (status.code(), false),
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                break (None, true);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => break (None, false),
        }
    };
    Ok(CmdOut { code, stdout: t_out.join().unwrap_or_default(), stderr: t_err.join().unwrap_or_default(), timed_out })
}

/// `git <args>` in `dir`; `Ok(stdout)` on exit 0, otherwise an error carrying
/// git's stderr (callers match on phrases like "already exists").
fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = run_git(dir, args, None)?;
    if out.code == Some(0) {
        Ok(out.stdout)
    } else {
        Err(format!("git {} failed: {}", args.join(" "), out.stderr.trim()))
    }
}

fn git_ok(dir: &Path, args: &[&str]) -> bool {
    run_git(dir, args, None).map(|o| o.code == Some(0)).unwrap_or(false)
}

/// Node's `path.resolve`: absolute, `.`/`..` folded lexically, no trailing
/// slash, symlinks *not* resolved.
fn resolve_path(p: &Path) -> PathBuf {
    let abs = std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    let mut out = PathBuf::new();
    for c in abs.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// Lexical equality first (the TS rule), then canonical equality so
/// `/tmp` vs `/private/tmp` style symlinked spellings still match.
fn same_path(a: &Path, b: &Path) -> bool {
    if resolve_path(a) == resolve_path(b) {
        return true;
    }
    matches!((a.canonicalize(), b.canonicalize()), (Ok(x), Ok(y)) if x == y)
}

// ---------------------------------------------------------------------------
// Branch / repo queries (git.ts)
// ---------------------------------------------------------------------------

/// `isGitRepo`: `<path>/.git` exists (a directory, or a file in a linked worktree).
pub fn is_git_repo(path: &Path) -> bool {
    path.join(".git").exists()
}

/// `getRepoName`: basename of the resolved path.
pub fn repo_name(path: &Path) -> String {
    resolve_path(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Where a new worktree for `branch` lives: `<repo>/.claude/worktrees/<branch
/// with every / replaced by ->`. Must match the TS app exactly.
pub fn worktree_path_for(repo_path: &Path, branch: &str) -> PathBuf {
    repo_path.join(".claude").join("worktrees").join(branch.replace('/', "-"))
}

/// `localBranchExists`: `refs/heads/<branch>` resolves.
pub fn local_branch_exists(repo_path: &Path, branch: &str) -> bool {
    git_ok(repo_path, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")])
}

/// `remoteBranchExists`: the *cached* remote-tracking ref
/// `refs/remotes/origin/<branch>` resolves (no network).
pub fn remote_branch_exists(repo_path: &Path, branch: &str) -> bool {
    git_ok(repo_path, &["rev-parse", "--verify", "--quiet", &format!("refs/remotes/origin/{branch}")])
}

/// `branchExists`: local, or cached on origin (`git worktree add -b` fails if
/// `origin/<branch>` exists, so both count).
pub fn branch_exists(repo_path: &Path, branch: &str) -> bool {
    local_branch_exists(repo_path, branch) || remote_branch_exists(repo_path, branch)
}

/// `getCurrentBranch`: `symbolic-ref --short HEAD` (works on empty repos),
/// falling back to `rev-parse --abbrev-ref HEAD` (`"HEAD"` when detached).
pub fn current_branch(repo_path: &Path) -> Result<String, String> {
    match git(repo_path, &["symbolic-ref", "--short", "HEAD"]) {
        Ok(b) => Ok(b.trim().to_string()),
        Err(_) => git(repo_path, &["rev-parse", "--abbrev-ref", "HEAD"]).map(|b| b.trim().to_string()),
    }
}

/// `getMainBranch`: `main` if it exists, else `master`, else the current branch.
pub fn main_branch(repo_path: &Path) -> Result<String, String> {
    for candidate in ["main", "master"] {
        if git_ok(repo_path, &["rev-parse", "--verify", "--quiet", candidate]) {
            return Ok(candidate.to_string());
        }
    }
    current_branch(repo_path)
}

/// `checkoutBranch`.
pub fn checkout_branch(repo_path: &Path, branch: &str) -> Result<(), String> {
    git(repo_path, &["checkout", branch])?;
    info!("git", "Checked out branch {branch} in {}", repo_path.display());
    Ok(())
}

/// `deleteBranch`: `git branch -d` (or `-D` with `force`).
pub fn delete_branch(repo_path: &Path, branch: &str, force: bool) -> Result<(), String> {
    git(repo_path, &["branch", if force { "-D" } else { "-d" }, branch])?;
    info!("git", "Deleted local branch {branch}");
    Ok(())
}

/// `deleteRemoteBranch`: `git push origin --delete <branch>`.
pub fn delete_remote_branch(repo_path: &Path, branch: &str) -> Result<(), String> {
    git(repo_path, &["push", "origin", "--delete", branch])?;
    info!("git", "Deleted remote branch origin/{branch}");
    Ok(())
}

/// `fetchBranch`: best effort `git fetch origin <branch>`; failures are logged
/// and ignored (offline creation still works from cached refs).
pub fn fetch_branch(repo_path: &Path, branch: &str) {
    match git(repo_path, &["fetch", "origin", branch]) {
        Ok(_) => info!("git", "Fetched origin/{branch}"),
        Err(e) => warn!("git", "Failed to fetch origin/{branch}: {e}"),
    }
}

/// `fetchAndResetBranch`: fetch `origin/<branch>` and force the local branch
/// to it (creating it if missing) with upstream set. Returns `false` (local
/// branch left as is) when the fetch fails or origin has no such branch.
pub fn fetch_and_reset_branch(repo_path: &Path, branch: &str) -> bool {
    if let Err(e) = git(repo_path, &["fetch", "origin", branch]) {
        warn!("git", "Failed to fetch origin/{branch} (using local branch as-is): {e}");
        return false;
    }
    if !remote_branch_exists(repo_path, branch) {
        debug!("git", "No remote branch origin/{branch}, using local as-is");
        return false;
    }
    let remote = format!("origin/{branch}");
    if let Err(e) = git(repo_path, &["branch", "-f", branch, &remote]) {
        debug!("git", "Failed to reset {branch} to {remote}, using local as-is: {e}");
        return false;
    }
    // `branch -f` doesn't configure upstream; set it so push/pull just work.
    if let Err(e) = git(repo_path, &["branch", "--set-upstream-to", &remote, branch]) {
        debug!("git", "Failed to set upstream for {branch}: {e}");
    }
    info!("git", "Reset local branch {branch} to {remote}");
    true
}

/// `recoverDetachedBranch`: a worktree mid-rebase shows as detached in
/// `git worktree list`; the branch being rebased is in
/// `<git-dir>/rebase-merge/head-name` (or `rebase-apply/head-name`).
pub fn recover_detached_branch(worktree_path: &Path) -> Option<String> {
    // Read from `.git` rather than `git rev-parse`: sync asks every 10 s.
    let Some((git_dir, _)) = crate::gitinfo::git_dirs(worktree_path) else {
        debug!("git", "Failed to recover detached branch for {}: no git dir", worktree_path.display());
        return None;
    };
    for dir in ["rebase-merge", "rebase-apply"] {
        if let Ok(r) = std::fs::read_to_string(git_dir.join(dir).join("head-name")) {
            let branch = r.trim().trim_start_matches("refs/heads/").to_string();
            debug!("git", "Recovered branch \"{branch}\" from {dir}/head-name for {}", worktree_path.display());
            return Some(branch);
        }
    }
    None
}

/// Detached worktrees already warned about (see [`list_git_worktrees`]).
static SKIPPED_DETACHED: std::sync::Mutex<std::collections::BTreeSet<String>> = std::sync::Mutex::new(std::collections::BTreeSet::new());

/// One entry of `git worktree list` with a usable branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitWorktree {
    pub path: String,
    pub branch: String,
    /// The repo's own checkout (path equals the repository path).
    pub is_main: bool,
}

/// `listWorktrees`: `git worktree list --porcelain` for `repo_path`. Detached
/// entries get their branch from [`recover_detached_branch`] or are skipped
/// (bare entries too). Unlike TS, a failing `git worktree list` is an error
/// rather than an empty list, so a transient failure can't make sync drop rows.
pub fn list_git_worktrees(repo_path: &Path) -> Result<Vec<GitWorktree>, String> {
    let out = git(repo_path, &["worktree", "list", "--porcelain"])?;
    let mut result = Vec::new();
    for (path, branch, _first) in crate::gitinfo::parse_worktree_list(&out) {
        let branch = match branch {
            Some(b) => b,
            None => match recover_detached_branch(Path::new(&path)) {
                Some(b) => b,
                None => {
                    // Sync runs every 10 s: say it once per path, not every time.
                    let first = SKIPPED_DETACHED.lock().map(|mut seen| seen.insert(path.clone())).unwrap_or(true);
                    if first {
                        warn!("git", "Skipping detached worktree at {path} (could not recover branch)");
                    } else {
                        debug!("git", "Skipping detached worktree at {path} (could not recover branch)");
                    }
                    continue;
                }
            },
        };
        let is_main = same_path(Path::new(&path), repo_path);
        result.push(GitWorktree { path, branch, is_main });
    }
    Ok(result)
}

// ---------------------------------------------------------------------------
// Remote-branch probe (git ls-remote vs gh api race)
// ---------------------------------------------------------------------------

/// How [`check_branch_with`] asks "is this branch on origin?".
#[derive(Clone)]
pub struct RemoteProbe {
    /// The `gh api` path; `None` races `git ls-remote` alone (tests, no gh).
    pub gh: Option<Arc<dyn GhRunner + Sync>>,
    /// An SSH handshake to github.com alone can take >3 s, so keep this generous.
    pub ls_remote_timeout: Duration,
    pub gh_timeout: Duration,
}

impl Default for RemoteProbe {
    fn default() -> Self {
        RemoteProbe {
            gh: Some(Arc::new(SystemGh)),
            ls_remote_timeout: Duration::from_secs(8),
            gh_timeout: Duration::from_secs(5),
        }
    }
}

impl RemoteProbe {
    /// `git ls-remote` only, no `gh`.
    pub fn git_only() -> Self {
        RemoteProbe { gh: None, ..Default::default() }
    }
}

/// `lsRemoteBranch`: authoritative check via `git ls-remote --exit-code`
/// (sees branches not yet fetched). On failure (no origin, offline, timeout)
/// falls back to the cached remote-tracking ref: a cached ref is a "yes", but
/// its absence is `None` (undetermined) rather than a "no".
pub fn ls_remote_branch(repo_path: &Path, branch: &str, timeout: Duration) -> Option<bool> {
    let refname = format!("refs/heads/{branch}");
    let args = ["ls-remote", "--exit-code", "--heads", "origin", refname.as_str()];
    let err = match run_git(repo_path, &args, Some(timeout)) {
        Ok(out) if out.code == Some(0) => return Some(!out.stdout.trim().is_empty()),
        // --exit-code returns 2 when no matching ref: an authoritative "no".
        Ok(out) if out.code == Some(2) => return Some(false),
        Ok(out) if out.timed_out => format!("timed out after {}s", timeout.as_secs()),
        Ok(out) => out.stderr.trim().to_string(),
        Err(e) => e,
    };
    warn!("git", "ls-remote failed for {branch} (falling back to cached ref): {err}");
    remote_branch_exists(repo_path, branch).then_some(true)
}

/// `ghBranchExists`: GitHub ref lookup over HTTPS (usually sub-second).
/// `Some(false)` only on an HTTP 404; any other failure is `None`.
pub fn gh_branch_exists(gh: &dyn GhRunner, repo_path: &Path, branch: &str, timeout: Duration) -> Option<bool> {
    let args = vec![
        "api".to_string(),
        format!("repos/{{owner}}/{{repo}}/git/ref/heads/{branch}"),
        "--jq".to_string(),
        ".ref".to_string(),
    ];
    let out = gh.run(repo_path, &args, timeout);
    if out.ok {
        return Some(out.stdout.trim() == format!("refs/heads/{branch}"));
    }
    if out.stderr.contains("HTTP 404") {
        debug!("github", "gh branch check: {branch} not on origin (404)");
        return Some(false);
    }
    debug!("github", "gh branch check inconclusive for {branch}: {}", out.stderr.trim());
    None
}

/// A boxed remote check for [`probe_race`].
pub type ProbeCheck = Box<dyn FnOnce() -> Option<bool> + Send>;

/// Runs every check on its own thread. The first `Some(true)` wins
/// immediately (slow checks are left to finish/time out in the background);
/// otherwise waits for all: any `Some(false)` → `Some(false)`, all
/// inconclusive → `None`. A failure or timeout on one path therefore never
/// produces a false "no".
pub fn probe_race(checks: Vec<ProbeCheck>) -> Option<bool> {
    let (tx, rx) = std::sync::mpsc::channel();
    let n = checks.len();
    for check in checks {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(check());
        });
    }
    drop(tx);
    let mut any_no = false;
    for _ in 0..n {
        match rx.recv() {
            Ok(Some(true)) => return Some(true),
            Ok(Some(false)) => any_no = true,
            Ok(None) => {}
            Err(_) => break, // a check panicked
        }
    }
    any_no.then_some(false)
}

/// `remoteBranchProbe` with an explicit probe configuration.
pub fn remote_branch_probe(repo_path: &Path, branch: &str, probe: &RemoteProbe) -> Option<bool> {
    let mut checks: Vec<ProbeCheck> = Vec::new();
    {
        let (repo, branch, timeout) = (repo_path.to_path_buf(), branch.to_string(), probe.ls_remote_timeout);
        checks.push(Box::new(move || ls_remote_branch(&repo, &branch, timeout)));
    }
    if let Some(gh) = probe.gh.clone() {
        let (repo, branch, timeout) = (repo_path.to_path_buf(), branch.to_string(), probe.gh_timeout);
        checks.push(Box::new(move || gh_branch_exists(gh.as_ref(), &repo, &branch, timeout)));
    }
    probe_race(checks)
}

/// Does `branch` already exist, locally and/or on origin?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BranchCheck {
    pub local: bool,
    /// `None` when neither `git ls-remote` nor `gh` could tell.
    pub remote: Option<bool>,
}

impl BranchCheck {
    /// Treats an undetermined remote as absent (the TS behaviour).
    pub fn remote_exists(&self) -> bool {
        self.remote == Some(true)
    }
    pub fn exists(&self) -> bool {
        self.local || self.remote_exists()
    }
}

/// Local + remote existence check using the default probe (`git ls-remote`
/// raced against `gh api`). Used by the New Worktree form's live check and by
/// [`create_worktree`].
pub fn check_branch(repo_path: &Path, branch: &str) -> BranchCheck {
    check_branch_with(repo_path, branch, &RemoteProbe::default())
}

pub fn check_branch_with(repo_path: &Path, branch: &str, probe: &RemoteProbe) -> BranchCheck {
    let check = std::thread::scope(|s| {
        let local = s.spawn(|| local_branch_exists(repo_path, branch));
        let remote = remote_branch_probe(repo_path, branch, probe);
        BranchCheck { local: local.join().unwrap_or(false), remote }
    });
    debug!("git", "Branch check {branch}: local={} remote={:?}", check.local, check.remote);
    check
}

// ---------------------------------------------------------------------------
// ensureBranchForOpen
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnsureBranch {
    /// Nothing to do.
    Ready,
    /// The main checkout was switched to the expected branch.
    Switched,
}

/// `ensureBranchForOpen`: before opening a *main-checkout* row whose branch
/// isn't the default branch, make sure the checkout is actually on it
/// (switching if clean). Non-main worktrees are always ready.
pub fn ensure_branch_for_open(worktree_path: &Path, expected_branch: &str, is_main: bool) -> Result<EnsureBranch, String> {
    if !is_main {
        return Ok(EnsureBranch::Ready);
    }
    if expected_branch == main_branch(worktree_path)? {
        return Ok(EnsureBranch::Ready);
    }
    if current_branch(worktree_path)? == expected_branch {
        return Ok(EnsureBranch::Ready);
    }
    if !branch_exists(worktree_path, expected_branch) {
        return Err(format!("Branch \"{expected_branch}\" does not exist."));
    }
    let dirty = crate::gitinfo::status(worktree_path).map(|s| s.dirty).unwrap_or(0);
    if dirty > 0 {
        let plural = if dirty == 1 { "" } else { "s" };
        return Err(format!(
            "Cannot switch to branch {expected_branch}: {dirty} uncommitted change{plural}. Stash or commit first."
        ));
    }
    checkout_branch(worktree_path, expected_branch)?;
    Ok(EnsureBranch::Switched)
}

// ---------------------------------------------------------------------------
// Sync (sync.ts)
// ---------------------------------------------------------------------------

/// What [`sync_repo`] changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncReport {
    /// Branches added to the DB.
    pub added: Vec<String>,
    /// Branches removed from the DB (gone from git, or renamed).
    pub removed: Vec<String>,
    /// Branches missing from `git worktree list` but kept because the path
    /// still has a `.git` (likely detached/rebasing).
    pub kept: Vec<String>,
    /// Number of worktrees git reported.
    pub git_worktrees: usize,
}

/// Default display name for a worktree row: the last `/` segment of the branch.
pub fn short_name(branch: &str) -> &str {
    branch.rsplit('/').next().unwrap_or(branch)
}

/// `syncWorktrees`: reconcile one repo's DB rows with `git worktree list`.
///
/// 1. Rows whose branch git no longer lists are removed — unless the path is
///    still a worktree (`<path>/.git` exists), except when git lists a
///    *different* branch at that path (a rename), which is always removed.
/// 2. Branches git lists but the DB lacks are inserted (`name` = last branch
///    segment, `is_main` from the path).
/// 3. Existing rows are upserted so moved paths / `is_main` stay current.
///    Nicknames (`custom_name`) are never touched.
pub fn sync_repo(conn: &Connection, repo: &Repository) -> Result<SyncReport, String> {
    let repo_path = Path::new(&repo.path);
    let git_wts = list_git_worktrees(repo_path)?;
    let db_wts = db::worktrees_for_repo(conn, &repo.id).map_err(|e| e.to_string())?;
    let mut report = SyncReport { git_worktrees: git_wts.len(), ..Default::default() };

    for dw in &db_wts {
        if git_wts.iter().any(|g| g.branch == dw.branch) {
            continue;
        }
        let renamed_to = git_wts.iter().find(|g| g.path == dw.path).map(|g| g.branch.as_str());
        if let Some(new_branch) = renamed_to {
            db::remove_worktree(conn, &dw.id).map_err(|e| e.to_string())?;
            info!("sync", "Removed worktree {} from DB (branch renamed to {new_branch})", dw.branch);
            report.removed.push(dw.branch.clone());
            continue;
        }
        if Path::new(&dw.path).join(".git").exists() {
            debug!("sync", "Keeping worktree {} (path exists, likely detached/rebasing)", dw.branch);
            report.kept.push(dw.branch.clone());
            continue;
        }
        db::remove_worktree(conn, &dw.id).map_err(|e| e.to_string())?;
        info!("sync", "Removed worktree {} from DB (no longer in git)", dw.branch);
        report.removed.push(dw.branch.clone());
    }

    // Rows that already match git aren't rewritten: sync runs every 10 s, and
    // each write is a commit that contends with hooks for the lock.
    let standalone = db::standalone_paths(conn).map_err(|e| e.to_string())?;
    for gw in &git_wts {
        let row = db_wts.iter().find(|d| d.branch == gw.branch);
        let name = short_name(&gw.branch);
        let unchanged = row.is_some_and(|d| d.path == gw.path && d.name == name && d.is_main == gw.is_main);
        if unchanged && !standalone.contains(&gw.path) {
            continue;
        }
        db::upsert_worktree(conn, &repo.id, &gw.path, &gw.branch, name, gw.is_main).map_err(|e| e.to_string())?;
        if row.is_none() {
            info!("sync", "Added worktree {} to DB{}", gw.branch, if gw.is_main { " (main)" } else { "" });
            report.added.push(gw.branch.clone());
        }
    }

    debug!("sync", "Synced worktrees for repo {}: {} in git", repo.name, git_wts.len());
    Ok(report)
}

/// Per-repository outcome of [`sync_all`].
pub type RepoSync = (Repository, Result<SyncReport, String>);

/// Sync every registered repository; one failing repo doesn't stop the rest.
pub fn sync_all(conn: &Connection) -> Result<Vec<RepoSync>, String> {
    let repos = db::repositories(conn).map_err(|e| e.to_string())?;
    Ok(repos
        .into_iter()
        .map(|r| {
            let res = sync_repo(conn, &r);
            if let Err(e) = &res {
                warn!("sync", "Sync failed for {}: {e}", r.name);
            }
            (r, res)
        })
        .collect())
}

// ---------------------------------------------------------------------------
// Repositories
// ---------------------------------------------------------------------------

/// `am repo add`: register the git repository at `path` (resolved like
/// Node's `path.resolve`) and sync its worktrees. An already-registered path
/// returns the existing row unchanged (no re-sync), as the TS command does.
pub fn add_repository(conn: &Connection, path: &Path) -> Result<Repository, String> {
    let abs = resolve_path(path);
    let abs_str = abs.to_string_lossy().into_owned();
    if !is_git_repo(&abs) {
        return Err(format!("Not a git repository: {abs_str}"));
    }
    if let Some(existing) = db::repository_by_path(conn, &abs_str).map_err(|e| e.to_string())? {
        debug!("repo", "Repository already tracked: {} ({})", existing.name, existing.path);
        return Ok(existing);
    }
    let repo = db::add_repository(conn, &abs_str, &repo_name(&abs)).map_err(|e| e.to_string())?;
    info!("repo", "Added repository {} ({})", repo.name, repo.path);
    if let Err(e) = sync_repo(conn, &repo) {
        warn!("repo", "Initial sync of {} failed: {e}", repo.name);
    }
    Ok(repo)
}

/// `am repo remove`: unregister a repository (its worktree rows cascade).
/// Nothing on disk is touched.
pub fn remove_repository(conn: &Connection, repo_id: &str) -> Result<(), String> {
    db::remove_repository(conn, repo_id).map_err(|e| e.to_string())?;
    info!("repo", "Removed repository {repo_id}");
    Ok(())
}

// ---------------------------------------------------------------------------
// Worktree limit (worktree-limit.ts)
// ---------------------------------------------------------------------------

/// `checkWorktreeLimit`: when `worktreeLimitEnabled`, block creation once the
/// repo has `maxWorktrees` dedicated worktrees (the main checkout excluded).
/// Returns the user-facing block message, or `None` when creation is allowed.
pub fn check_worktree_limit(settings: &Settings, conn: &Connection, repo_id: &str, repo_name: &str) -> Option<String> {
    if !settings.worktree_limit_enabled {
        return None;
    }
    let dedicated = match db::worktrees_for_repo(conn, repo_id) {
        // Tmp-space worktrees (someone else's branch pulled down to look at)
        // don't count.
        Ok(wts) => wts.iter().filter(|w| !w.is_main && !w.tmp_space).count(),
        Err(e) => {
            warn!("worktree-limit", "Could not count worktrees for {repo_name}: {e}");
            return None;
        }
    };
    let max = settings.max_worktrees as usize;
    if dedicated < max {
        return None;
    }
    info!("worktree-limit", "Blocked worktree creation for {repo_name}: {dedicated}/{max} dedicated worktrees");
    Some(format!(
        "Worktree limit reached ({dedicated}/{max}) for {repo_name}.\n\
         Complete or delete an existing worktree before creating a new one.\n\
         Adjust or disable the limit in Settings."
    ))
}

// ---------------------------------------------------------------------------
// Create
// ---------------------------------------------------------------------------

/// How to treat an existing branch (the TUI's `BranchExistsPrompt` choices /
/// the CLI's `--reuse` / `--track` / `--no-track`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CreateMode {
    /// New branch from the base. Fails with [`CreateError::BranchExists`] if
    /// the branch exists locally or on origin, or the worktree dir exists.
    #[default]
    Fresh,
    /// Attach to the existing local branch, first fast-forwarding it to
    /// `origin/<branch>` when origin has it (`--reuse`).
    ReuseLocal,
    /// Pull the remote branch: reset+reuse the local branch if there is one,
    /// otherwise create it tracking `origin/<branch>` (`--track`).
    TrackRemote,
    /// New branch from the base with no upstream, deleting any local branch
    /// of that name first (`--no-track`).
    Disconnected,
    /// Delete the local branch, then create fresh from the base.
    DeleteAndRecreate,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CreateRequest {
    pub branch: String,
    /// Base for new branches; `None`/empty → `main`, `master`, or the current
    /// branch. `origin/<base>` is used when it exists after a fetch.
    pub base_branch: Option<String>,
    /// Optional nickname stored as the row's `custom_name`.
    pub custom_name: Option<String>,
    pub mode: CreateMode,
    /// Label the worktree "tmp space": shown in the shared `Tmp space` group
    /// and exempt from the worktree limit.
    pub tmp_space: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateError {
    /// Fresh mode only: pick another [`CreateMode`] and retry.
    BranchExists { local: bool, remote: bool, path_exists: bool },
    LimitReached(String),
    /// Request can't work in the current state (empty name, `TrackRemote`
    /// without a remote branch).
    Invalid(String),
    Git(String),
    Db(String),
}

impl std::fmt::Display for CreateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CreateError::BranchExists { local, remote, path_exists } => {
                let where_ = match (local, remote) {
                    (true, true) => "locally and on origin",
                    (false, true) => "on origin",
                    (true, false) => "locally",
                    (false, false) => "",
                };
                if where_.is_empty() && *path_exists {
                    write!(f, "Worktree directory already exists")
                } else {
                    write!(f, "Branch already exists {where_}")
                }
            }
            CreateError::LimitReached(m) | CreateError::Invalid(m) | CreateError::Git(m) | CreateError::Db(m) => {
                f.write_str(m)
            }
        }
    }
}

impl std::error::Error for CreateError {}

fn db_err(e: rusqlite::Error) -> CreateError {
    CreateError::Db(e.to_string())
}

/// Create a worktree using the default remote probe (`git ls-remote` + `gh`).
/// See [`create_worktree_with`].
pub fn create_worktree(
    conn: &Connection,
    settings: &Settings,
    repo: &Repository,
    req: &CreateRequest,
) -> Result<Worktree, CreateError> {
    create_worktree_with(conn, settings, repo, req, &RemoteProbe::default())
}

/// [`create_worktree`] that reports each stage ("Fetching origin/main…")
/// to `progress`, for the TUI's jobs pane.
pub fn create_worktree_progress(
    conn: &Connection,
    settings: &Settings,
    repo: &Repository,
    req: &CreateRequest,
    progress: &mut dyn FnMut(&str),
) -> Result<Worktree, CreateError> {
    create_worktree_inner(conn, settings, repo, req, &RemoteProbe::default(), progress)
}

/// The TUI's `doCreateWorktree` / `am worktree create`: limit check →
/// existence check (fresh only) → optional local-branch delete → fetch →
/// `git worktree add` at [`worktree_path_for`] → sync → nickname. Running the
/// repo's startup script ([`script_path`]) is left to the caller.
pub fn create_worktree_with(
    conn: &Connection,
    settings: &Settings,
    repo: &Repository,
    req: &CreateRequest,
    probe: &RemoteProbe,
) -> Result<Worktree, CreateError> {
    create_worktree_inner(conn, settings, repo, req, probe, &mut |_| {})
}

fn create_worktree_inner(
    conn: &Connection,
    settings: &Settings,
    repo: &Repository,
    req: &CreateRequest,
    probe: &RemoteProbe,
    progress: &mut dyn FnMut(&str),
) -> Result<Worktree, CreateError> {
    let mut stage = |text: String| {
        debug!("git", "Create {}: {text}", req.branch.trim());
        progress(&text);
    };
    let branch = req.branch.trim();
    if branch.is_empty() {
        return Err(CreateError::Invalid("Branch name is required".into()));
    }
    if !req.tmp_space
        && let Some(msg) = check_worktree_limit(settings, conn, &repo.id, &repo.name)
    {
        return Err(CreateError::LimitReached(msg));
    }
    let repo_path = Path::new(&repo.path);
    let wt_path = worktree_path_for(repo_path, branch);
    let branch_exists_err = |check: BranchCheck| CreateError::BranchExists {
        local: check.local,
        remote: check.remote_exists(),
        path_exists: wt_path.exists(),
    };

    // The tmp space checks out someone else's branch: never a new one.
    if req.tmp_space && matches!(req.mode, CreateMode::Disconnected | CreateMode::DeleteAndRecreate) {
        return Err(CreateError::Invalid("Reviewing only takes an existing branch as it is".into()));
    }
    if req.mode == CreateMode::Fresh {
        stage("Checking branch…".into());
        let check = check_branch_with(repo_path, branch, probe);
        if check.exists() || wt_path.exists() {
            return Err(branch_exists_err(check));
        }
        if req.tmp_space {
            debug!("git", "Tmp-space create of {branch} refused: no such branch (remote={:?})", check.remote);
            return Err(CreateError::Invalid(format!(
                "No branch \"{branch}\" locally or on origin: Reviewing only takes existing branches"
            )));
        }
    }

    let deletes_local = matches!(req.mode, CreateMode::Disconnected | CreateMode::DeleteAndRecreate);
    if deletes_local && local_branch_exists(repo_path, branch) {
        stage("Deleting local branch…".into());
        if let Err(e) = delete_branch(repo_path, branch, true) {
            debug!("git", "Local branch delete failed (may not exist locally): {e}");
        }
    }

    // Decide the `git worktree add` arguments.
    let path_str = wt_path.to_string_lossy().into_owned();
    let mut args: Vec<String> = vec!["worktree".into(), "add".into(), path_str.clone()];
    let reuse = |args: &mut Vec<String>| args.push(branch.to_string());
    match req.mode {
        CreateMode::ReuseLocal => {
            stage(format!("Fetching origin/{branch}…"));
            fetch_and_reset_branch(repo_path, branch);
            reuse(&mut args);
        }
        CreateMode::TrackRemote => {
            stage(format!("Fetching origin/{branch}…"));
            if local_branch_exists(repo_path, branch) {
                fetch_and_reset_branch(repo_path, branch);
                reuse(&mut args);
            } else {
                fetch_branch(repo_path, branch);
                if !remote_branch_exists(repo_path, branch) {
                    return Err(CreateError::Invalid(format!(
                        "Cannot track: branch \"{branch}\" does not exist on origin."
                    )));
                }
                args.extend(["-b".into(), branch.into(), "--track".into(), format!("origin/{branch}")]);
            }
        }
        CreateMode::Fresh | CreateMode::Disconnected | CreateMode::DeleteAndRecreate => {
            let base = match req.base_branch.as_deref().map(str::trim).filter(|b| !b.is_empty()) {
                Some(b) => b.to_string(),
                None => main_branch(repo_path).map_err(CreateError::Git)?,
            };
            stage(format!("Fetching origin/{base}…"));
            fetch_branch(repo_path, &base);
            let base_ref = if remote_branch_exists(repo_path, &base) { format!("origin/{base}") } else { base };
            args.extend(["-b".into(), branch.into()]);
            if req.mode == CreateMode::Disconnected {
                // --no-track: no upstream even if origin/<branch> exists
                // (git's DWIM would otherwise auto-track).
                args.push("--no-track".into());
            }
            args.push(base_ref);
        }
    }

    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    stage("Adding worktree…".into());
    if let Err(e) = git(repo_path, &arg_refs) {
        if e.contains("already exists") || e.contains("already checked out") {
            warn!("git", "Worktree add for {branch} hit an existing branch/path: {e}");
            return Err(branch_exists_err(check_branch_with(repo_path, branch, probe)));
        }
        return Err(CreateError::Git(e));
    }
    info!("git", "Created worktree at {path_str} for branch {branch} ({:?})", req.mode);

    stage("Syncing…".into());
    sync_repo(conn, repo).map_err(CreateError::Git)?;
    let wt = db::worktree_by_branch(conn, &repo.id, branch)
        .map_err(db_err)?
        .ok_or_else(|| CreateError::Db(format!("Worktree {branch} missing from DB after sync")))?;
    let name = req.custom_name.as_deref().map(str::trim).filter(|n| !n.is_empty());
    if name.is_none() && wt.tmp_space == req.tmp_space {
        return Ok(wt);
    }
    if let Some(name) = name {
        stage("Setting nickname…".into());
        db::update_worktree_custom_name(conn, &wt.id, Some(name), None).map_err(db_err)?;
    }
    // Set either way: a row kept from an earlier worktree of this branch may
    // still carry the old label.
    if wt.tmp_space != req.tmp_space {
        if req.tmp_space {
            stage("Moving to Reviewing…".into());
        }
        db::set_worktree_tmp_space(conn, &wt.id, req.tmp_space).map_err(db_err)?;
    }
    db::worktree_by_branch(conn, &repo.id, branch)
        .map_err(db_err)?
        .ok_or_else(|| CreateError::Db(format!("Worktree {branch} vanished")))
}

// ---------------------------------------------------------------------------
// Delete
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DeleteOptions {
    /// Also delete the local branch (always done for branch-only deletes).
    pub delete_branch: bool,
    /// Also `git push origin --delete` the branch, if origin has it.
    pub delete_remote: bool,
    /// `git worktree remove --force` (falling back to `rm -rf` + prune) and
    /// `git branch -D` instead of `-d`.
    pub force: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteKind {
    /// A dedicated worktree directory was removed.
    Worktree,
    /// A main-checkout row on a feature branch: switched the checkout to the
    /// default branch and deleted the feature branch.
    BranchOnly,
    /// The worktree directory was already gone: cleaned up branch + row.
    Stale,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteReport {
    pub kind: DeleteKind,
    /// The path actually removed (may differ from the DB path when git had the
    /// branch checked out elsewhere).
    pub path: String,
    pub branch_deleted: bool,
    pub remote_deleted: bool,
    /// Non-fatal failures (branch / remote branch deletion).
    pub warnings: Vec<String>,
}

/// Delete a worktree row and what backs it (the TUI's `handleDelete` /
/// `am worktree delete`):
///
/// - If the DB path is gone but git has the branch live at another path, that
///   path is deleted instead (otherwise sync would re-add the row).
/// - A main-checkout row on a non-`main`/`master` branch is *branch-only*:
///   check out the default branch, delete the branch. A row whose path is
///   gone is *stale*: delete the branch (failure ignored), drop the row.
///   Both re-sync the repo afterwards.
/// - Otherwise `git worktree remove` the directory, optionally delete the
///   branch (failure is a warning), and drop the row.
/// - Deleting the main working tree itself is refused.
///
/// On `Err` from the worktree removal step the DB row is left in place; the
/// TUI's recovery path is `db::remove_worktree` + optional [`delete_branch`].
pub fn delete_worktree(conn: &Connection, wt: &Worktree, opts: &DeleteOptions) -> Result<DeleteReport, String> {
    let repo = db::repository_by_id(conn, &wt.repo_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Repository not found in DB.".to_string())?;
    let repo_path = Path::new(&repo.path);

    let mut live_path = wt.path.clone();
    let mut path_exists = Path::new(&wt.path).exists();
    if !path_exists {
        match list_git_worktrees(repo_path) {
            Ok(gws) => {
                if let Some(live) = gws.into_iter().find(|g| g.branch == wt.branch && !g.is_main) {
                    info!(
                        "worktrees",
                        "Redirecting delete of {}: DB path {} is missing, branch is live at {}",
                        wt.branch,
                        wt.path,
                        live.path
                    );
                    live_path = live.path;
                    path_exists = true;
                }
            }
            Err(e) => warn!("worktrees", "Failed to list git worktrees while resolving delete target: {e}"),
        }
    }

    let is_default = wt.branch == "main" || wt.branch == "master";
    let is_stale = !path_exists && !wt.is_main;
    let branch_only = (wt.is_main && !is_default) || is_stale;

    if !branch_only && same_path(Path::new(&live_path), repo_path) {
        return Err("Cannot delete the main working tree".into());
    }

    let mut report = DeleteReport {
        kind: if is_stale {
            DeleteKind::Stale
        } else if branch_only {
            DeleteKind::BranchOnly
        } else {
            DeleteKind::Worktree
        },
        path: live_path.clone(),
        branch_deleted: false,
        remote_deleted: false,
        warnings: Vec::new(),
    };

    if branch_only {
        if !is_stale {
            let main = main_branch(repo_path)?;
            checkout_branch(repo_path, &main)
                .map_err(|e| format!("Failed to switch to {main}. Commit or stash changes first.\n{e}"))?;
        }
        match delete_branch(repo_path, &wt.branch, opts.force) {
            Ok(()) => report.branch_deleted = true,
            Err(e) if is_stale => debug!("worktrees", "Branch delete failed (may already be gone): {e}"),
            Err(e) => return Err(format!("Failed to delete local branch: {e}")),
        }
        delete_remote_if_requested(repo_path, &wt.branch, opts, &mut report);
        db::remove_worktree(conn, &wt.id).map_err(|e| e.to_string())?;
        if let Err(e) = sync_repo(conn, &repo) {
            report.warnings.push(format!("Sync failed: {e}"));
        }
        info!("worktrees", "Deleted {:?} entry {}", report.kind, wt.branch);
        return Ok(report);
    }

    remove_worktree_dir(repo_path, Path::new(&live_path), opts.force)?;

    if opts.delete_branch {
        match delete_branch(repo_path, &wt.branch, opts.force) {
            Ok(()) => report.branch_deleted = true,
            Err(e) => {
                warn!("worktrees", "Failed to delete local branch: {e}");
                report.warnings.push(format!("Failed to delete local branch: {e}"));
            }
        }
    }
    delete_remote_if_requested(repo_path, &wt.branch, opts, &mut report);
    db::remove_worktree(conn, &wt.id).map_err(|e| e.to_string())?;
    info!("worktrees", "Deleted worktree {} ({live_path})", wt.branch);
    Ok(report)
}

fn delete_remote_if_requested(repo_path: &Path, branch: &str, opts: &DeleteOptions, report: &mut DeleteReport) {
    if !opts.delete_remote || !remote_branch_exists(repo_path, branch) {
        return;
    }
    match delete_remote_branch(repo_path, branch) {
        Ok(()) => report.remote_deleted = true,
        Err(e) => {
            warn!("worktrees", "Failed to delete remote branch: {e}");
            report.warnings.push(format!("Failed to delete remote branch: {e}"));
        }
    }
}

/// `deleteWorktree` (git.ts): `git worktree remove [--force]`. With `force`,
/// a failure while the directory still exists (e.g. "Directory not empty"
/// from untracked `.claude/` dirs) falls back to `rm -rf` + `git worktree prune`.
pub fn remove_worktree_dir(repo_path: &Path, worktree_path: &Path, force: bool) -> Result<(), String> {
    let path = worktree_path.to_string_lossy();
    let mut args = vec!["worktree", "remove", path.as_ref()];
    if force {
        args.push("--force");
    }
    if let Err(e) = git(repo_path, &args) {
        if force && worktree_path.exists() {
            warn!("git", "git worktree remove failed, removing directory manually: {e}");
            std::fs::remove_dir_all(worktree_path).map_err(|e| format!("Failed to remove {path}: {e}"))?;
            git(repo_path, &["worktree", "prune"])?;
        } else {
            return Err(e);
        }
    }
    info!("git", "Deleted worktree at {path}");
    Ok(())
}

// ---------------------------------------------------------------------------
// Startup scripts (scripts.ts)
// ---------------------------------------------------------------------------

const SCRIPT_TEMPLATE: &str = "#!/bin/bash
# Startup script for worktree creation
# This runs automatically after a new worktree is created.
# The working directory is set to the new worktree path.

# Example: install dependencies
# npm install

# Example: set up environment
# cp .env.example .env

echo \"Startup script finished.\"
";

/// `<APP_DIR>/scripts`.
pub fn scripts_dir() -> PathBuf {
    crate::paths::app_dir().join("scripts")
}

/// `<APP_DIR>/scripts/<repo-id>.sh` (the file may not exist).
pub fn script_path(repo_id: &str) -> PathBuf {
    script_path_in(&scripts_dir(), repo_id)
}

/// `hasStartupScript`.
pub fn has_script(repo_id: &str) -> bool {
    script_path(repo_id).exists()
}

/// The script's contents, or `None` when the repo has none.
pub fn read_script(repo_id: &str) -> Option<String> {
    std::fs::read_to_string(script_path(repo_id)).ok()
}

/// Write (create or replace) the script, mode 0755. Returns its path.
pub fn write_script(repo_id: &str, content: &str) -> std::io::Result<PathBuf> {
    write_script_in(&scripts_dir(), repo_id, content)
}

/// `createStartupScript`: write the commented template if no script exists
/// yet (an existing script is left alone). Returns the path.
pub fn create_script(repo_id: &str) -> std::io::Result<PathBuf> {
    create_script_in(&scripts_dir(), repo_id)
}

/// `removeStartupScript`. `Ok(false)` when there was none.
pub fn remove_script(repo_id: &str) -> std::io::Result<bool> {
    remove_script_in(&scripts_dir(), repo_id)
}

fn script_path_in(dir: &Path, repo_id: &str) -> PathBuf {
    dir.join(format!("{repo_id}.sh"))
}

fn write_script_in(dir: &Path, repo_id: &str, content: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = script_path_in(dir, repo_id);
    std::fs::write(&path, content)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
    }
    info!("scripts", "Wrote startup script: {}", path.display());
    Ok(path)
}

fn create_script_in(dir: &Path, repo_id: &str) -> std::io::Result<PathBuf> {
    let path = script_path_in(dir, repo_id);
    if path.exists() {
        return Ok(path);
    }
    write_script_in(dir, repo_id, SCRIPT_TEMPLATE)
}

fn remove_script_in(dir: &Path, repo_id: &str) -> std::io::Result<bool> {
    let path = script_path_in(dir, repo_id);
    match std::fs::remove_file(&path) {
        Ok(()) => {
            info!("scripts", "Removed startup script: {}", path.display());
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrations::github::GhOutput;

    /// Run git in `dir` for test setup, isolated from the user's config.
    fn sh(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "commit.gpgsign=false", "-c", "user.name=t", "-c", "user.email=t@t", "-c", "init.defaultBranch=main"])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// A repo with one commit on `main`, pushed to a bare `origin`, plus a
    /// temp DB with the repo registered.
    struct Fx {
        _tmp: tempfile::TempDir,
        root: PathBuf,
        repo_path: PathBuf,
        conn: Connection,
        repo: Repository,
    }

    impl Fx {
        fn new() -> Fx {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().canonicalize().unwrap();
            let repo_path = root.join("repo");
            let origin = root.join("origin.git");
            std::fs::create_dir_all(&repo_path).unwrap();
            std::fs::create_dir_all(&origin).unwrap();
            sh(&origin, &["init", "--bare", "-b", "main"]);
            sh(&repo_path, &["init", "-b", "main"]);
            std::fs::write(repo_path.join("README"), "hi\n").unwrap();
            sh(&repo_path, &["add", "."]);
            sh(&repo_path, &["commit", "-m", "init"]);
            sh(&repo_path, &["remote", "add", "origin", origin.to_str().unwrap()]);
            sh(&repo_path, &["push", "-u", "origin", "main"]);
            let conn = db::open_at(&root.join("t.db")).unwrap();
            let repo = add_repository(&conn, &repo_path).unwrap();
            Fx { _tmp: tmp, root, repo_path, conn, repo }
        }

        /// Push a branch to origin that doesn't exist locally (pushed from a
        /// second clone), one commit ahead of main.
        fn remote_only_branch(&self, branch: &str) {
            let other = self.root.join(format!("other-{}", branch.replace('/', "-")));
            sh(&self.root, &["clone", self.root.join("origin.git").to_str().unwrap(), other.to_str().unwrap()]);
            sh(&other, &["checkout", "-b", branch]);
            std::fs::write(other.join("remote.txt"), "r\n").unwrap();
            sh(&other, &["add", "."]);
            sh(&other, &["commit", "-m", "remote work"]);
            sh(&other, &["push", "origin", branch]);
        }

        fn create(&self, branch: &str, mode: CreateMode) -> Result<Worktree, CreateError> {
            self.create_with(&Settings::default(), branch, mode)
        }

        fn create_with(&self, settings: &Settings, branch: &str, mode: CreateMode) -> Result<Worktree, CreateError> {
            let req = CreateRequest { branch: branch.into(), mode, ..Default::default() };
            create_worktree_with(&self.conn, settings, &self.repo, &req, &RemoteProbe::git_only())
        }

        fn rows(&self) -> Vec<Worktree> {
            db::worktrees_for_repo(&self.conn, &self.repo.id).unwrap()
        }

        fn upstream(&self, dir: &Path, branch: &str) -> Option<String> {
            run_git(dir, &["rev-parse", "--abbrev-ref", &format!("{branch}@{{upstream}}")], None)
                .ok()
                .filter(|o| o.code == Some(0))
                .map(|o| o.stdout.trim().to_string())
        }
    }

    // --- repositories + sync ---

    #[test]
    fn add_repository_registers_and_syncs_main_checkout() {
        let fx = Fx::new();
        assert_eq!(fx.repo.name, "repo");
        let rows = fx.rows();
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].branch.as_str(), rows[0].is_main), ("main", true));
        // Re-adding returns the existing row.
        let again = add_repository(&fx.conn, &fx.repo_path.join("sub/..")).unwrap();
        assert_eq!(again.id, fx.repo.id);
        // Not a git repo.
        assert!(add_repository(&fx.conn, &fx.root).unwrap_err().contains("Not a git repository"));
        remove_repository(&fx.conn, &fx.repo.id).unwrap();
        assert!(db::repositories(&fx.conn).unwrap().is_empty());
        assert!(fx.rows().is_empty());
    }

    #[test]
    fn sync_adds_removes_and_detects_renames() {
        let fx = Fx::new();
        let a = fx.root.join("wt-a");
        let b = fx.root.join("wt-b");
        sh(&fx.repo_path, &["worktree", "add", a.to_str().unwrap(), "-b", "feature/a"]);
        sh(&fx.repo_path, &["worktree", "add", b.to_str().unwrap(), "-b", "feature/b"]);
        let r = sync_repo(&fx.conn, &fx.repo).unwrap();
        assert_eq!(r.added, vec!["feature/a", "feature/b"]);
        assert_eq!(r.git_worktrees, 3);
        let rows = fx.rows();
        let row_a = rows.iter().find(|w| w.branch == "feature/a").unwrap();
        assert_eq!((row_a.name.as_str(), row_a.is_main), ("a", false));
        db::update_worktree_custom_name(&fx.conn, &row_a.id, Some("Nick"), None).unwrap();

        // Rename feature/a in place → old row removed, new branch added.
        sh(&a, &["branch", "-m", "feature/a2"]);
        // Remove feature/b via git → row removed (path gone).
        sh(&fx.repo_path, &["worktree", "remove", b.to_str().unwrap()]);
        let r = sync_repo(&fx.conn, &fx.repo).unwrap();
        let mut removed = r.removed.clone();
        removed.sort();
        assert_eq!(removed, vec!["feature/a", "feature/b"]);
        assert_eq!(r.added, vec!["feature/a2"]);
        let mut branches: Vec<String> = fx.rows().into_iter().map(|w| w.branch).collect();
        branches.sort();
        assert_eq!(branches, vec!["feature/a2", "main"]);

        // Idempotent: a second sync changes nothing and keeps nicknames.
        let r = sync_repo(&fx.conn, &fx.repo).unwrap();
        assert!(r.added.is_empty() && r.removed.is_empty());
    }

    #[test]
    fn sync_keeps_nickname_and_updates_moved_path() {
        let fx = Fx::new();
        let a = fx.root.join("wt-a");
        sh(&fx.repo_path, &["worktree", "add", a.to_str().unwrap(), "-b", "feat"]);
        sync_repo(&fx.conn, &fx.repo).unwrap();
        let row = db::worktree_by_branch(&fx.conn, &fx.repo.id, "feat").unwrap().unwrap();
        db::update_worktree_custom_name(&fx.conn, &row.id, Some("Nick"), None).unwrap();
        let moved = fx.root.join("wt-moved");
        sh(&fx.repo_path, &["worktree", "move", a.to_str().unwrap(), moved.to_str().unwrap()]);
        sync_repo(&fx.conn, &fx.repo).unwrap();
        let row2 = db::worktree_by_branch(&fx.conn, &fx.repo.id, "feat").unwrap().unwrap();
        assert_eq!(row2.id, row.id);
        assert_eq!(row2.path, moved.to_string_lossy());
        assert_eq!(row2.custom_name.as_deref(), Some("Nick"));
    }

    #[test]
    fn sync_writes_nothing_when_git_matches_the_db() {
        let fx = Fx::new();
        let a = fx.root.join("wt-a");
        sh(&fx.repo_path, &["worktree", "add", a.to_str().unwrap(), "-b", "feat"]);
        let detached = fx.root.join("wt-d");
        sh(&fx.repo_path, &["worktree", "add", "--detach", detached.to_str().unwrap()]);
        sync_repo(&fx.conn, &fx.repo).unwrap();
        let before = fx.conn.total_changes();
        for _ in 0..3 {
            sync_repo(&fx.conn, &fx.repo).unwrap();
        }
        assert_eq!(fx.conn.total_changes(), before, "an unchanged sync must not write");

        // A standalone session at a tracked path is still promoted.
        fx.conn
            .execute(
                "INSERT INTO standalone_sessions (path, status, updated_at) VALUES (?1, 'executing', datetime('now'))",
                [a.to_str().unwrap()],
            )
            .unwrap();
        sync_repo(&fx.conn, &fx.repo).unwrap();
        assert!(db::standalone_paths(&fx.conn).unwrap().is_empty());
    }

    #[test]
    fn sync_keeps_detached_worktree_whose_path_still_exists() {
        let fx = Fx::new();
        let a = fx.root.join("wt-a");
        sh(&fx.repo_path, &["worktree", "add", a.to_str().unwrap(), "-b", "feat"]);
        sync_repo(&fx.conn, &fx.repo).unwrap();
        sh(&a, &["checkout", "--detach"]);
        // Plain detach: no rebase state, so git lists no branch and recovery fails.
        assert_eq!(recover_detached_branch(&a), None);
        let r = sync_repo(&fx.conn, &fx.repo).unwrap();
        assert_eq!(r.kept, vec!["feat"]);
        assert!(fx.rows().iter().any(|w| w.branch == "feat"));
    }

    #[test]
    fn detached_branch_is_recovered_from_rebase_state() {
        let fx = Fx::new();
        let a = fx.root.join("wt-a");
        sh(&fx.repo_path, &["worktree", "add", a.to_str().unwrap(), "-b", "feature/rebasing"]);
        sh(&a, &["checkout", "--detach"]);
        let git_dir = PathBuf::from(sh(&a, &["rev-parse", "--absolute-git-dir"]));
        // rebase-apply only (rebase-merge missing) still recovers.
        std::fs::create_dir_all(git_dir.join("rebase-apply")).unwrap();
        std::fs::write(git_dir.join("rebase-apply/head-name"), "refs/heads/feature/rebasing\n").unwrap();
        assert_eq!(recover_detached_branch(&a).as_deref(), Some("feature/rebasing"));
        let listed = list_git_worktrees(&fx.repo_path).unwrap();
        assert!(listed.iter().any(|g| g.branch == "feature/rebasing" && !g.is_main));
        assert!(listed.iter().any(|g| g.branch == "main" && g.is_main));
        std::fs::remove_dir_all(git_dir.join("rebase-apply")).unwrap();
    }

    #[test]
    fn sync_fails_without_touching_rows_when_git_fails() {
        let fx = Fx::new();
        let bogus = Repository { id: fx.repo.id.clone(), path: fx.root.join("nope").to_string_lossy().into(), name: "x".into() };
        assert!(sync_repo(&fx.conn, &bogus).is_err());
        assert_eq!(fx.rows().len(), 1);
        let all = sync_all(&fx.conn).unwrap();
        assert_eq!(all.len(), 1);
        assert!(all[0].1.is_ok());
    }

    // --- branch checks ---

    #[test]
    fn check_branch_local_and_remote() {
        let fx = Fx::new();
        let p = RemoteProbe::git_only();
        assert_eq!(check_branch_with(&fx.repo_path, "main", &p), BranchCheck { local: true, remote: Some(true) });
        assert_eq!(check_branch_with(&fx.repo_path, "nope", &p), BranchCheck { local: false, remote: Some(false) });
        fx.remote_only_branch("feature/r");
        // Not fetched yet: ls-remote still sees it.
        assert!(!remote_branch_exists(&fx.repo_path, "feature/r"));
        assert_eq!(check_branch_with(&fx.repo_path, "feature/r", &p), BranchCheck { local: false, remote: Some(true) });
        sh(&fx.repo_path, &["branch", "local-only"]);
        let c = check_branch_with(&fx.repo_path, "local-only", &p);
        assert_eq!(c, BranchCheck { local: true, remote: Some(false) });
        assert!(c.exists());
    }

    #[test]
    fn ls_remote_without_origin_falls_back_to_cached_ref() {
        let fx = Fx::new();
        sh(&fx.repo_path, &["remote", "set-url", "origin", fx.root.join("missing.git").to_str().unwrap()]);
        // origin/main is cached → yes; an uncached branch is undetermined, not "no".
        assert_eq!(ls_remote_branch(&fx.repo_path, "main", Duration::from_secs(5)), Some(true));
        assert_eq!(ls_remote_branch(&fx.repo_path, "other", Duration::from_secs(5)), None);
    }

    #[test]
    fn probe_race_first_yes_wins_and_failures_never_mean_no() {
        let hung: ProbeCheck = Box::new(|| {
            std::thread::sleep(Duration::from_secs(5));
            None
        });
        let yes: ProbeCheck = Box::new(|| Some(true));
        let start = Instant::now();
        assert_eq!(probe_race(vec![hung, yes]), Some(true));
        assert!(start.elapsed() < Duration::from_secs(2), "did not wait for the hung check");

        let b = |v: Option<bool>| -> ProbeCheck { Box::new(move || v) };
        assert_eq!(probe_race(vec![b(Some(false)), b(Some(false))]), Some(false));
        assert_eq!(probe_race(vec![b(Some(false)), b(None)]), Some(false));
        assert_eq!(probe_race(vec![b(None), b(Some(true))]), Some(true));
        assert_eq!(probe_race(vec![b(None), b(None)]), None);
    }

    struct FakeGh(GhOutput);
    impl GhRunner for FakeGh {
        fn run(&self, _cwd: &Path, args: &[String], _t: Duration) -> GhOutput {
            assert_eq!(args[1], "repos/{owner}/{repo}/git/ref/heads/feature/x");
            self.0.clone()
        }
    }

    #[test]
    fn gh_branch_exists_classifies_responses() {
        let p = Path::new("/");
        let t = Duration::from_secs(1);
        assert_eq!(gh_branch_exists(&FakeGh(GhOutput::success("refs/heads/feature/x\n")), p, "feature/x", t), Some(true));
        let nf = GhOutput::failure("gh: Not Found (HTTP 404)");
        assert_eq!(gh_branch_exists(&FakeGh(nf), p, "feature/x", t), Some(false));
        let down = GhOutput::failure("gh timed out after 5s");
        assert_eq!(gh_branch_exists(&FakeGh(down), p, "feature/x", t), None);
    }

    #[test]
    fn probe_uses_gh_yes_when_git_cannot_tell() {
        let fx = Fx::new();
        sh(&fx.repo_path, &["remote", "set-url", "origin", fx.root.join("missing.git").to_str().unwrap()]);
        let probe = RemoteProbe {
            gh: Some(Arc::new(FakeGh(GhOutput::success("refs/heads/feature/x")))),
            ..RemoteProbe::git_only()
        };
        assert_eq!(remote_branch_probe(&fx.repo_path, "feature/x", &probe), Some(true));
    }

    #[test]
    fn main_branch_detection() {
        let fx = Fx::new();
        assert_eq!(main_branch(&fx.repo_path).unwrap(), "main");
        sh(&fx.repo_path, &["branch", "-m", "main", "master"]);
        assert_eq!(main_branch(&fx.repo_path).unwrap(), "master");
        sh(&fx.repo_path, &["branch", "-m", "master", "trunk"]);
        assert_eq!(main_branch(&fx.repo_path).unwrap(), "trunk");
        // Empty repo: symbolic-ref works before the first commit.
        let empty = fx.root.join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        sh(&empty, &["init", "-b", "dev"]);
        assert_eq!(current_branch(&empty).unwrap(), "dev");
    }

    // --- create ---

    #[test]
    fn create_fresh_worktree_at_ts_location() {
        let fx = Fx::new();
        let req = CreateRequest {
            branch: "feature/new-thing".into(),
            custom_name: Some("My thing".into()),
            ..Default::default()
        };
        let wt = create_worktree_with(&fx.conn, &Settings::default(), &fx.repo, &req, &RemoteProbe::git_only()).unwrap();
        let expected = fx.repo_path.join(".claude/worktrees/feature-new-thing");
        assert_eq!(worktree_path_for(&fx.repo_path, "feature/new-thing"), expected);
        assert_eq!(wt.path, expected.to_string_lossy());
        assert!(expected.join("README").exists());
        assert_eq!((wt.name.as_str(), wt.is_main), ("new-thing", false));
        assert_eq!(wt.custom_name.as_deref(), Some("My thing"));
        // Based on origin/main (git's default autoSetupMerge tracks it, as in TS).
        assert_eq!(sh(&expected, &["rev-parse", "HEAD"]), sh(&fx.repo_path, &["rev-parse", "origin/main"]));
        assert_eq!(fx.rows().len(), 2);
    }

    #[test]
    fn create_fresh_reports_existing_branch_cases() {
        let fx = Fx::new();
        sh(&fx.repo_path, &["branch", "local-only"]);
        assert_eq!(
            fx.create("local-only", CreateMode::Fresh).unwrap_err(),
            CreateError::BranchExists { local: true, remote: false, path_exists: false }
        );
        fx.remote_only_branch("remote-only");
        assert_eq!(
            fx.create("remote-only", CreateMode::Fresh).unwrap_err(),
            CreateError::BranchExists { local: false, remote: true, path_exists: false }
        );
        std::fs::create_dir_all(worktree_path_for(&fx.repo_path, "dir/taken")).unwrap();
        assert_eq!(
            fx.create("dir/taken", CreateMode::Fresh).unwrap_err(),
            CreateError::BranchExists { local: false, remote: false, path_exists: true }
        );
        assert!(matches!(fx.create("  ", CreateMode::Fresh), Err(CreateError::Invalid(_))));
        assert_eq!(fx.rows().len(), 1);
    }

    #[test]
    fn create_track_remote_without_local_branch() {
        let fx = Fx::new();
        fx.remote_only_branch("feature/r");
        let wt = fx.create("feature/r", CreateMode::TrackRemote).unwrap();
        let p = PathBuf::from(&wt.path);
        assert!(p.join("remote.txt").exists());
        assert_eq!(fx.upstream(&fx.repo_path, "feature/r").as_deref(), Some("origin/feature/r"));
    }

    #[test]
    fn create_track_remote_resets_existing_local_branch() {
        let fx = Fx::new();
        fx.remote_only_branch("both");
        sh(&fx.repo_path, &["branch", "both"]); // stale local copy at main
        assert_eq!(
            fx.create("both", CreateMode::Fresh).unwrap_err(),
            CreateError::BranchExists { local: true, remote: true, path_exists: false }
        );
        let wt = fx.create("both", CreateMode::TrackRemote).unwrap();
        assert!(Path::new(&wt.path).join("remote.txt").exists(), "local was reset to origin");
        assert_eq!(fx.upstream(&fx.repo_path, "both").as_deref(), Some("origin/both"));
    }

    #[test]
    fn create_track_remote_errors_when_origin_lacks_branch() {
        let fx = Fx::new();
        assert!(matches!(fx.create("ghost", CreateMode::TrackRemote), Err(CreateError::Invalid(_))));
    }

    #[test]
    fn create_reuse_local_attaches_existing_branch() {
        let fx = Fx::new();
        sh(&fx.repo_path, &["checkout", "-b", "mine"]);
        std::fs::write(fx.repo_path.join("mine.txt"), "m\n").unwrap();
        sh(&fx.repo_path, &["add", "."]);
        sh(&fx.repo_path, &["commit", "-m", "mine"]);
        sh(&fx.repo_path, &["checkout", "main"]);
        let wt = fx.create("mine", CreateMode::ReuseLocal).unwrap();
        assert!(Path::new(&wt.path).join("mine.txt").exists());
        assert_eq!(fx.upstream(&fx.repo_path, "mine"), None);
    }

    #[test]
    fn create_disconnected_deletes_local_and_has_no_upstream() {
        let fx = Fx::new();
        fx.remote_only_branch("dis");
        sh(&fx.repo_path, &["fetch", "origin"]);
        sh(&fx.repo_path, &["branch", "dis", "origin/dis"]);
        let wt = fx.create("dis", CreateMode::Disconnected).unwrap();
        assert!(!Path::new(&wt.path).join("remote.txt").exists(), "recreated from main");
        assert_eq!(fx.upstream(&fx.repo_path, "dis"), None);
    }

    #[test]
    fn create_delete_and_recreate_starts_from_base() {
        let fx = Fx::new();
        sh(&fx.repo_path, &["checkout", "-b", "redo"]);
        std::fs::write(fx.repo_path.join("old.txt"), "o\n").unwrap();
        sh(&fx.repo_path, &["add", "."]);
        sh(&fx.repo_path, &["commit", "-m", "old"]);
        sh(&fx.repo_path, &["checkout", "main"]);
        let wt = fx.create("redo", CreateMode::DeleteAndRecreate).unwrap();
        assert!(!Path::new(&wt.path).join("old.txt").exists());
    }

    #[test]
    fn create_uses_custom_base_branch() {
        let fx = Fx::new();
        sh(&fx.repo_path, &["checkout", "-b", "develop"]);
        std::fs::write(fx.repo_path.join("dev.txt"), "d\n").unwrap();
        sh(&fx.repo_path, &["add", "."]);
        sh(&fx.repo_path, &["commit", "-m", "dev"]);
        sh(&fx.repo_path, &["checkout", "main"]);
        let req = CreateRequest { branch: "on-dev".into(), base_branch: Some("develop".into()), ..Default::default() };
        let wt = create_worktree_with(&fx.conn, &Settings::default(), &fx.repo, &req, &RemoteProbe::git_only()).unwrap();
        assert!(Path::new(&wt.path).join("dev.txt").exists(), "local base used when origin lacks it");
    }

    #[test]
    fn create_blocked_by_limit() {
        let fx = Fx::new();
        let settings = Settings { worktree_limit_enabled: true, max_worktrees: 1, ..Settings::default() };
        fx.create_with(&settings, "one", CreateMode::Fresh).unwrap();
        match fx.create_with(&settings, "two", CreateMode::Fresh) {
            Err(CreateError::LimitReached(m)) => assert!(m.contains("1/1") && m.contains("repo")),
            other => panic!("expected limit, got {other:?}"),
        }
    }

    #[test]
    fn tmp_space_create_is_labelled_and_skips_the_limit() {
        let fx = Fx::new();
        let settings = Settings { worktree_limit_enabled: true, max_worktrees: 1, ..Settings::default() };
        fx.create_with(&settings, "one", CreateMode::Fresh).unwrap();
        let req = CreateRequest { branch: "theirs/fix".into(), tmp_space: true, ..Default::default() };
        let probe = RemoteProbe::git_only();
        // Only existing branches: a new one is refused, in any mode that makes one.
        match create_worktree_with(&fx.conn, &settings, &fx.repo, &req, &probe) {
            Err(CreateError::Invalid(m)) => assert!(m.contains("only takes existing branches"), "{m}"),
            other => panic!("expected refusal, got {other:?}"),
        }
        let disconnected = CreateRequest { mode: CreateMode::Disconnected, ..req.clone() };
        assert!(matches!(create_worktree_with(&fx.conn, &settings, &fx.repo, &disconnected, &probe), Err(CreateError::Invalid(_))));
        sh(&fx.repo_path, &["branch", "theirs/fix"]);
        assert!(matches!(
            create_worktree_with(&fx.conn, &settings, &fx.repo, &req, &probe),
            Err(CreateError::BranchExists { local: true, remote: false, .. })
        ));
        let req = CreateRequest { mode: CreateMode::ReuseLocal, ..req };
        let wt = create_worktree_with(&fx.conn, &settings, &fx.repo, &req, &probe).unwrap();
        assert!(wt.tmp_space);
        // Re-syncing keeps the label, and a tmp worktree doesn't count.
        sync_repo(&fx.conn, &fx.repo).unwrap();
        assert!(fx.rows().iter().any(|w| w.branch == "theirs/fix" && w.tmp_space));
        db::set_worktree_tmp_space(&fx.conn, &fx.rows().iter().find(|w| w.branch == "one").unwrap().id, true).unwrap();
        fx.create_with(&settings, "two", CreateMode::Fresh).unwrap();
        assert!(!fx.rows().iter().find(|w| w.branch == "two").unwrap().tmp_space);
    }

    // --- delete ---

    #[test]
    fn delete_worktree_and_branch() {
        let fx = Fx::new();
        let wt = fx.create("feature/del", CreateMode::Fresh).unwrap();
        let opts = DeleteOptions { delete_branch: true, force: true, ..Default::default() };
        let r = delete_worktree(&fx.conn, &wt, &opts).unwrap();
        assert_eq!(r.kind, DeleteKind::Worktree);
        assert!(r.branch_deleted);
        assert!(!Path::new(&wt.path).exists());
        assert!(!local_branch_exists(&fx.repo_path, "feature/del"));
        assert_eq!(fx.rows().len(), 1);
    }

    #[test]
    fn delete_dirty_worktree_needs_force() {
        let fx = Fx::new();
        let wt = fx.create("dirty", CreateMode::Fresh).unwrap();
        std::fs::write(Path::new(&wt.path).join("scratch.txt"), "x").unwrap();
        assert!(delete_worktree(&fx.conn, &wt, &DeleteOptions::default()).is_err());
        assert_eq!(fx.rows().len(), 2, "row kept when removal fails");
        let r = delete_worktree(&fx.conn, &wt, &DeleteOptions { force: true, ..Default::default() }).unwrap();
        assert!(!r.branch_deleted);
        assert!(local_branch_exists(&fx.repo_path, "dirty"), "branch kept unless requested");
        assert!(!Path::new(&wt.path).exists());
    }

    #[test]
    fn delete_remote_branch_when_requested() {
        let fx = Fx::new();
        let wt = fx.create("pushed", CreateMode::Fresh).unwrap();
        sh(Path::new(&wt.path), &["push", "-u", "origin", "pushed"]);
        let opts = DeleteOptions { delete_branch: true, delete_remote: true, force: true };
        let r = delete_worktree(&fx.conn, &wt, &opts).unwrap();
        assert!(r.remote_deleted);
        assert_eq!(ls_remote_branch(&fx.repo_path, "pushed", Duration::from_secs(5)), Some(false));
    }

    #[test]
    fn delete_stale_entry_cleans_branch_and_row() {
        let fx = Fx::new();
        let wt = fx.create("stale", CreateMode::Fresh).unwrap();
        sh(&fx.repo_path, &["worktree", "remove", &wt.path]);
        let r = delete_worktree(&fx.conn, &wt, &DeleteOptions::default()).unwrap();
        assert_eq!(r.kind, DeleteKind::Stale);
        assert!(r.branch_deleted);
        assert_eq!(fx.rows().len(), 1);
    }

    #[test]
    fn delete_redirects_to_live_path() {
        let fx = Fx::new();
        let wt = fx.create("moved", CreateMode::Fresh).unwrap();
        let elsewhere = fx.root.join("elsewhere");
        sh(&fx.repo_path, &["worktree", "move", &wt.path, elsewhere.to_str().unwrap()]);
        let r = delete_worktree(&fx.conn, &wt, &DeleteOptions { force: true, ..Default::default() }).unwrap();
        assert_eq!(r.kind, DeleteKind::Worktree);
        assert_eq!(r.path, elsewhere.to_string_lossy());
        assert!(!elsewhere.exists());
    }

    #[test]
    fn delete_branch_only_for_main_checkout_on_feature_branch() {
        let fx = Fx::new();
        sh(&fx.repo_path, &["checkout", "-b", "feat-in-main"]);
        sync_repo(&fx.conn, &fx.repo).unwrap();
        let row = db::worktree_by_branch(&fx.conn, &fx.repo.id, "feat-in-main").unwrap().unwrap();
        assert!(row.is_main);
        let r = delete_worktree(&fx.conn, &row, &DeleteOptions::default()).unwrap();
        assert_eq!(r.kind, DeleteKind::BranchOnly);
        assert_eq!(current_branch(&fx.repo_path).unwrap(), "main");
        assert!(!local_branch_exists(&fx.repo_path, "feat-in-main"));
        let rows = fx.rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].branch, "main");
        assert!(fx.repo_path.exists());
    }

    #[test]
    fn delete_refuses_main_working_tree() {
        let fx = Fx::new();
        let main = fx.rows().remove(0);
        let err = delete_worktree(&fx.conn, &main, &DeleteOptions { force: true, ..Default::default() }).unwrap_err();
        assert!(err.contains("main working tree"));
        assert!(fx.repo_path.join("README").exists());
    }

    // --- ensureBranchForOpen ---

    #[test]
    fn ensure_branch_for_open_cases() {
        let fx = Fx::new();
        let p = &fx.repo_path;
        assert_eq!(ensure_branch_for_open(p, "whatever", false).unwrap(), EnsureBranch::Ready);
        assert_eq!(ensure_branch_for_open(p, "main", true).unwrap(), EnsureBranch::Ready);
        assert_eq!(ensure_branch_for_open(p, "ghost", true).unwrap_err(), "Branch \"ghost\" does not exist.");
        sh(p, &["branch", "feat"]);
        std::fs::write(p.join("README"), "changed\n").unwrap();
        assert_eq!(
            ensure_branch_for_open(p, "feat", true).unwrap_err(),
            "Cannot switch to branch feat: 1 uncommitted change. Stash or commit first."
        );
        std::fs::write(p.join("other"), "x").unwrap();
        assert!(ensure_branch_for_open(p, "feat", true).unwrap_err().contains("2 uncommitted changes."));
        sh(p, &["checkout", "--", "README"]);
        std::fs::remove_file(p.join("other")).unwrap();
        assert_eq!(ensure_branch_for_open(p, "feat", true).unwrap(), EnsureBranch::Switched);
        assert_eq!(current_branch(p).unwrap(), "feat");
        assert_eq!(ensure_branch_for_open(p, "feat", true).unwrap(), EnsureBranch::Ready);
    }

    // --- worktree limit (ported from worktree-limit.test.ts) ---

    fn limit_db() -> (tempfile::TempDir, Connection, String) {
        let dir = tempfile::tempdir().unwrap();
        let conn = db::open_at(&dir.path().join("t.db")).unwrap();
        let id = db::add_repository(&conn, "/tmp/limit-repo", "limit-repo").unwrap().id;
        (dir, conn, id)
    }

    fn add_rows(conn: &Connection, repo_id: &str, n: usize, prefix: &str) {
        for i in 0..n {
            db::upsert_worktree(conn, repo_id, &format!("/tmp/{prefix}/wt-{i}"), &format!("feature/{i}"), "x", false)
                .unwrap();
        }
    }

    fn limit(enabled: bool, max: u32) -> Settings {
        Settings { worktree_limit_enabled: enabled, max_worktrees: max, ..Settings::default() }
    }

    #[test]
    fn worktree_limit_rules() {
        let (_d, conn, id) = limit_db();
        add_rows(&conn, &id, 3, "limit-repo");
        assert_eq!(check_worktree_limit(&limit(true, 5), &conn, &id, "limit-repo"), None);
        add_rows(&conn, &id, 10, "limit-repo");
        assert_eq!(check_worktree_limit(&limit(false, 5), &conn, &id, "limit-repo"), None, "disabled");
        let msg = check_worktree_limit(&limit(true, 10), &conn, &id, "limit-repo").unwrap();
        assert!(msg.contains("10/10") && msg.contains("limit-repo"));
        assert!(check_worktree_limit(&limit(true, 5), &conn, &id, "limit-repo").unwrap().contains("10/5"));
    }

    #[test]
    fn worktree_limit_excludes_main_and_counts_per_repo() {
        let (_d, conn, id) = limit_db();
        add_rows(&conn, &id, 4, "limit-repo");
        db::upsert_worktree(&conn, &id, "/tmp/limit-repo", "main", "main", true).unwrap();
        assert_eq!(check_worktree_limit(&limit(true, 5), &conn, &id, "limit-repo"), None);
        let other = db::add_repository(&conn, "/tmp/other-repo", "other-repo").unwrap().id;
        add_rows(&conn, &other, 10, "other-repo");
        assert_eq!(check_worktree_limit(&limit(true, 5), &conn, &id, "limit-repo"), None);
    }

    // --- scripts ---

    #[test]
    fn scripts_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("scripts");
        assert_eq!(script_path_in(&dir, "abc"), dir.join("abc.sh"));
        assert!(!remove_script_in(&dir, "abc").unwrap());
        let p = create_script_in(&dir, "abc").unwrap();
        assert!(std::fs::read_to_string(&p).unwrap().starts_with("#!/bin/bash\n# Startup script"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o755);
        }
        write_script_in(&dir, "abc", "echo hi\n").unwrap();
        // create never overwrites an existing script.
        create_script_in(&dir, "abc").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "echo hi\n");
        assert!(remove_script_in(&dir, "abc").unwrap());
        assert!(!p.exists());
        assert!(script_path("r1").ends_with("scripts/r1.sh"));
    }

    #[test]
    fn short_names_and_paths() {
        assert_eq!(short_name("feature/foo/bar"), "bar");
        assert_eq!(short_name("main"), "main");
        assert_eq!(resolve_path(Path::new("/a/b/../c/./d/")), PathBuf::from("/a/c/d"));
        assert_eq!(repo_name(Path::new("/x/my-repo/")), "my-repo");
    }
}
