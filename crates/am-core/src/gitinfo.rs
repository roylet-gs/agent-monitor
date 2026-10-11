//! Per-worktree git state with two spawns instead of the TS app's four:
//! `git status --porcelain=v2 --branch` gives ahead/behind *and* the dirty
//! count, `git log -1` gives the last commit with a raw timestamp.

use std::path::Path;
use std::process::{Command, Stdio};

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
    let mut s = GitStatus::default();
    for line in porcelain_v2.lines() {
        if let Some(ab) = line.strip_prefix("# branch.ab ") {
            let mut parts = ab.split_whitespace();
            s.ahead = parts.next().and_then(|a| a.trim_start_matches('+').parse().ok()).unwrap_or(0);
            s.behind = parts.next().and_then(|b| b.trim_start_matches('-').parse().ok()).unwrap_or(0);
        } else if !line.starts_with('#') && !line.starts_with('!') && !line.is_empty() {
            s.dirty += 1;
        }
    }
    s
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
        assert!(last_commit(here).is_some());
    }
}
