//! Resolve CLI targets (branch names, paths, repos) to DB entities.
//! Port of `src/lib/resolve.ts`; same messages and exit codes.

use std::path::{Component, Path, PathBuf};

use am_core::model::Worktree;
use rusqlite::Connection;

use crate::fail;
use crate::store::{self, RepoRow};

/// Node's `path.resolve`: absolute against the CWD, `.`/`..` folded lexically,
/// symlinks untouched.
pub fn abs_path(p: &str) -> String {
    let p = Path::new(p);
    let joined = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")).join(p)
    };
    let mut out = PathBuf::from("/");
    for c in joined.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(s) => out.push(s),
            Component::RootDir | Component::CurDir | Component::Prefix(_) => {}
        }
    }
    out.to_string_lossy().into_owned()
}

/// Walk up from `start` to a tracked repo root. Also accepts a tracked linked
/// worktree on the way (the TS version only matched repo roots, so it missed
/// worktrees that live outside the main checkout).
pub fn detect_repo(conn: &Connection, start: &str) -> Option<RepoRow> {
    let worktrees = store::worktrees(conn);
    let repos = store::repositories(conn);
    let mut dir = PathBuf::from(abs_path(start));
    loop {
        let d = dir.to_string_lossy();
        if dir.join(".git").exists() {
            if let Some(r) = repos.iter().find(|r| r.path == d) {
                return Some(r.clone());
            }
            if let Some(w) = worktrees.iter().find(|w| w.path == d)
                && let Some(r) = repos.iter().find(|r| r.id == w.repo_id)
            {
                return Some(r.clone());
            }
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// `--repo <path>` or detect from the CWD; exits 1 when neither works.
pub fn resolve_repo(conn: &Connection, repo_path: Option<&str>) -> RepoRow {
    if let Some(p) = repo_path {
        let abs = abs_path(p);
        return store::repository_by_path(conn, &abs)
            .unwrap_or_else(|| fail(&format!("Repository not tracked: {abs}\nRun: am repo add {abs}")));
    }
    let cwd = std::env::current_dir().map(|d| d.to_string_lossy().into_owned()).unwrap_or_else(|_| "/".into());
    detect_repo(conn, &cwd).unwrap_or_else(|| {
        fail("Could not detect repository from CWD.\nSpecify --repo <path> or run from inside a tracked repo.")
    })
}

/// A worktree by path, exact branch, or `…/<target>` / name suffix.
pub fn resolve_worktree(conn: &Connection, target: &str, repo_id: Option<&str>) -> Worktree {
    let abs = abs_path(target);
    let all = store::worktrees(conn);
    if let Some(w) = all.iter().find(|w| w.path == abs) {
        if repo_id.is_some_and(|id| id != w.repo_id) {
            fail(&format!("Worktree {target} belongs to a different repository."));
        }
        return w.clone();
    }
    let scoped: Vec<&Worktree> = all.iter().filter(|w| repo_id.is_none_or(|id| id == w.repo_id)).collect();

    let by_branch: Vec<&&Worktree> = scoped.iter().filter(|w| w.branch == target).collect();
    match by_branch.len() {
        1 => return (*by_branch[0]).clone(),
        n if n > 1 => fail(&format!(
            "Ambiguous target \"{target}\" — matches {n} worktrees. Specify --repo or use a path."
        )),
        _ => {}
    }

    let suffix = format!("/{target}");
    let partial: Vec<&&Worktree> = scoped.iter().filter(|w| w.branch.ends_with(&suffix) || w.name == target).collect();
    match partial.len() {
        1 => (*partial[0]).clone(),
        n if n > 1 => fail(&format!("Ambiguous target \"{target}\" — matches {n} worktrees. Be more specific.")),
        _ => fail(&format!("Worktree not found: {target}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abs_path_folds_dots() {
        assert_eq!(abs_path("/a/b/../c/./d"), "/a/c/d");
        assert_eq!(abs_path("/"), "/");
        assert_eq!(abs_path("/a/"), "/a");
    }
}
