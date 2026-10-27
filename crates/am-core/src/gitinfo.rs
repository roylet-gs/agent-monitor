//! Per-worktree git state with two spawns instead of the TS app's four:
//! `git status --porcelain=v2 --branch` gives ahead/behind *and* the dirty
//! count, `git log -1` gives the last commit with a raw timestamp. [`refresh`]
//! skips the second spawn when the status shows the same commit, and
//! [`signature`] tells (from file metadata, no spawn) whether anything git
//! tracks about the worktree could have changed since last time.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::SystemTime;

use crate::model::{CommitInfo, GitStatus};

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0") // never take index.lock just to refresh stat info
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn status(dir: &Path) -> Option<GitStatus> {
    let out = git(dir, &["status", "--porcelain=v2", "--branch", "--untracked-files=normal"])?;
    Some(parse_status(&out))
}

pub fn parse_status(porcelain_v2: &str) -> GitStatus {
    parse_status_oid(porcelain_v2).0
}

/// [`parse_status`] plus the `# branch.oid` (`None` before the first commit).
pub fn parse_status_oid(porcelain_v2: &str) -> (GitStatus, Option<String>) {
    let mut s = GitStatus::default();
    let mut oid = None;
    for line in porcelain_v2.lines() {
        if let Some(o) = line.strip_prefix("# branch.oid ") {
            oid = (o != "(initial)").then(|| o.trim().to_string());
        } else if let Some(ab) = line.strip_prefix("# branch.ab ") {
            let mut parts = ab.split_whitespace();
            s.ahead = parts.next().and_then(|a| a.trim_start_matches('+').parse().ok()).unwrap_or(0);
            s.behind = parts.next().and_then(|b| b.trim_start_matches('-').parse().ok()).unwrap_or(0);
        } else if !line.starts_with('#') && !line.starts_with('!') && !line.is_empty() {
            s.dirty += 1;
        }
    }
    (s, oid)
}

/// Status and last commit, reusing `prev` (no `git log`) when HEAD still
/// points at it.
pub fn refresh(dir: &Path, prev: Option<&CommitInfo>) -> (Option<GitStatus>, Option<CommitInfo>) {
    let Some(out) = git(dir, &["status", "--porcelain=v2", "--branch", "--untracked-files=normal"]) else {
        return (None, last_commit(dir));
    };
    let (status, oid) = parse_status_oid(&out);
    let commit = match (prev, oid) {
        (Some(p), Some(oid)) if !p.hash.is_empty() && oid.starts_with(&p.hash) => Some(p.clone()),
        _ => last_commit(dir),
    };
    (Some(status), commit)
}

/// A worktree's git dir and the repo's common dir, read from `.git` (a
/// directory in the main checkout, a `gitdir:` file in a linked worktree)
/// without spawning git.
pub fn git_dirs(worktree: &Path) -> Option<(PathBuf, PathBuf)> {
    let dot = worktree.join(".git");
    if dot.is_dir() {
        return Some((dot.clone(), dot));
    }
    let text = std::fs::read_to_string(&dot).ok()?;
    let dir = worktree.join(text.trim().strip_prefix("gitdir:")?.trim()); // an absolute gitdir replaces the base
    let common = match std::fs::read_to_string(dir.join("commondir")) {
        Ok(c) => dir.join(c.trim()),
        Err(_) => dir.clone(),
    };
    Some((dir, common))
}

/// What [`signature`] looks at, each `(mtime, len)` or `None` when missing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature(Vec<Option<(SystemTime, u64)>>);

/// File metadata that changes whenever git's view of the worktree does,
/// short of edits to the working tree itself: HEAD (checkout, commit,
/// rebase), the index (add, commit, reset, status refreshing stat info), the
/// branch and its `origin/` ref (commit, fetch, push) and packed-refs. `None`
/// when the git dir can't be found. Stat calls only, no spawn.
pub fn signature(worktree: &Path) -> Option<Signature> {
    let (dir, common) = git_dirs(worktree)?;
    let stat = |p: PathBuf| std::fs::metadata(p).ok().and_then(|m| Some((m.modified().ok()?, m.len())));
    let head = std::fs::read_to_string(dir.join("HEAD")).unwrap_or_default();
    let branch = head.trim().strip_prefix("ref: refs/heads/").map(str::to_string);
    let mut parts = vec![stat(dir.join("HEAD")), stat(dir.join("index")), stat(common.join("packed-refs"))];
    if let Some(b) = branch {
        parts.push(stat(common.join("refs/heads").join(&b)));
        parts.push(stat(common.join("refs/remotes/origin").join(&b)));
    }
    Some(Signature(parts))
}

pub fn last_commit(dir: &Path) -> Option<CommitInfo> {
    let out = git(dir, &["log", "-1", "--format=%h%x1f%ct%x1f%s"])?;
    parse_last_commit(&out)
}

pub fn parse_last_commit(out: &str) -> Option<CommitInfo> {
    let mut parts = out.trim_end_matches('\n').splitn(3, '\x1f');
    let hash = parts.next()?.to_string();
    let timestamp = parts.next()?.parse().ok()?;
    let message = parts.next().unwrap_or("").to_string();
    (!hash.is_empty()).then_some(CommitInfo { hash, message, timestamp })
}

/// `git worktree list --porcelain` for a repo: (path, branch, is_main).
pub fn list_worktrees(repo: &Path) -> Option<Vec<(String, Option<String>, bool)>> {
    let out = git(repo, &["worktree", "list", "--porcelain"])?;
    Some(parse_worktree_list(&out))
}

/// Entries are keyed off each `worktree ` line rather than blank-line
/// separators, so output missing a separator can't merge two entries (the TS
/// parser had the same guard). `branch` is `None` for detached/bare entries.
pub fn parse_worktree_list(out: &str) -> Vec<(String, Option<String>, bool)> {
    let mut result: Vec<(String, Option<String>, bool)> = Vec::new();
    for line in out.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            let first = result.is_empty();
            result.push((p.trim().to_string(), None, first));
        } else if let Some(b) = line.strip_prefix("branch ")
            && let Some(last) = result.last_mut()
        {
            last.1 = Some(b.trim().trim_start_matches("refs/heads/").to_string());
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn porcelain_v2_ahead_behind_and_dirty() {
        let out = "# branch.oid abc\n# branch.head feat\n# branch.upstream origin/feat\n# branch.ab +2 -1\n1 .M N... 100644 100644 100644 a b src/x.rs\n? new.txt\n! ignored\n";
        assert_eq!(parse_status(out), GitStatus { ahead: 2, behind: 1, dirty: 2 });
    }

    #[test]
    fn porcelain_v2_oid() {
        assert_eq!(parse_status_oid("# branch.oid abc123\n# branch.head feat\n").1.as_deref(), Some("abc123"));
        assert_eq!(parse_status_oid("# branch.oid (initial)\n").1, None);
    }

    #[test]
    fn no_upstream_is_zero_ahead_behind() {
        assert_eq!(parse_status("# branch.oid abc\n# branch.head feat\n"), GitStatus::default());
    }

    #[test]
    fn last_commit_with_separator_in_subject() {
        let c = parse_last_commit("eaa3440\x1f1760170000\x1fchore: bump version to 1.0.95\n").unwrap();
        assert_eq!((c.hash.as_str(), c.timestamp, c.message.as_str()), ("eaa3440", 1760170000, "chore: bump version to 1.0.95"));
        assert!(parse_last_commit("").is_none());
    }

    #[test]
    fn worktree_list() {
        let out = "worktree /r\nHEAD a\nbranch refs/heads/main\n\nworktree /r/.claude/worktrees/x\nHEAD b\nbranch refs/heads/feature/x\n\nworktree /r/d\nHEAD c\ndetached\n";
        let l = parse_worktree_list(out);
        assert_eq!(l[0], ("/r".into(), Some("main".into()), true));
        assert_eq!(l[1].1.as_deref(), Some("feature/x"));
        assert_eq!(l[2].1, None);
    }

    #[test]
    fn worktree_list_without_blank_separators() {
        let out = "worktree /r\nHEAD a\nbranch refs/heads/main\nworktree /r/x\nHEAD b\nbranch refs/heads/x";
        let l = parse_worktree_list(out);
        assert_eq!(l.len(), 2);
        assert_eq!(l[1], ("/r/x".into(), Some("x".into()), false));
    }

    #[test]
    fn real_repo_smoke() {
        let here = Path::new(env!("CARGO_MANIFEST_DIR"));
        assert!(status(here).is_some());
        let commit = last_commit(here);
        assert!(commit.is_some());
        let (st, again) = refresh(here, commit.as_ref());
        assert!(st.is_some());
        assert_eq!(again, commit);
        // A stale hash is not reused.
        let stale = CommitInfo { hash: "0000000".into(), message: "old".into(), timestamp: 0 };
        assert_ne!(refresh(here, Some(&stale)).1.as_ref(), Some(&stale));
    }

    fn sh(dir: &Path, args: &[&str]) {
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
    }

    #[test]
    fn signature_follows_commits_in_main_and_linked_worktrees() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("r");
        std::fs::create_dir(&repo).unwrap();
        sh(&repo, &["init", "-q"]);
        sh(&repo, &["commit", "-q", "--allow-empty", "-m", "one"]);
        let linked = tmp.path().join("wt");
        sh(&repo, &["worktree", "add", "-q", "-b", "feature/x", linked.to_str().unwrap()]);

        let (dir, common) = git_dirs(&linked).unwrap();
        assert!(dir.ends_with(".git/worktrees/wt"), "{}", dir.display());
        assert_eq!(common.canonicalize().unwrap(), repo.join(".git").canonicalize().unwrap());
        assert_eq!(git_dirs(&repo).unwrap().0, repo.join(".git"));
        assert!(git_dirs(tmp.path()).is_none());

        for wt in [&repo, &linked] {
            let before = signature(wt).unwrap();
            assert_eq!(signature(wt).unwrap(), before, "stable while nothing changes");
            // mtimes have 1 s resolution on some filesystems: let the clock move.
            std::thread::sleep(std::time::Duration::from_millis(1_100));
            sh(wt, &["commit", "-q", "--allow-empty", "-m", "two"]);
            assert_ne!(signature(wt).unwrap(), before, "a commit in {}", wt.display());
        }
    }
}
