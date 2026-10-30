//! Stacked branches: which worktree's branch is built on top of another's.
//!
//! Two signals, PR first:
//! 1. The branch's PR targets another worktree's branch (Graphite / gh stack /
//!    manual stacks — also survives squash-rebases).
//! 2. Git: another worktree branch's tip is among this branch's commits that
//!    aren't on the trunk yet. The closest such branch (most unmerged commits)
//!    is the parent.
//!
//! A stack's root can also be built on a branch that isn't a worktree:
//! [`base_branch`] names it (the PR's base, else the closest `origin/*`
//! branch in its unmerged commits) so the dashboard can say so.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::{Command, Stdio};

/// Cap on commits inspected per branch; stacks deeper than this are unusual.
const MAX_COMMITS: usize = 300;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    pub worktree_id: String,
    pub name: String,
    pub is_main: bool,
}

/// What git knows about each branch: its tip and the commits not on trunk.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitView {
    pub tips: HashMap<String, String>,
    pub unmerged: HashMap<String, HashSet<String>>,
    /// The same commits, newest first (`git rev-list` order).
    pub order: HashMap<String, Vec<String>>,
}

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Branch tips for the whole repo in one call (`refs/heads`).
pub fn branch_tips(repo: &Path) -> HashMap<String, String> {
    git(repo, &["for-each-ref", "--format=%(refname:short) %(objectname)", "refs/heads"])
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.split_once(' ').map(|(b, s)| (b.to_string(), s.to_string())))
        .collect()
}

/// Trunk to measure "unmerged" against: `origin/<main>` when it exists.
pub fn trunk_ref(repo: &Path, main: &str) -> String {
    let remote = format!("origin/{main}");
    if git(repo, &["rev-parse", "--verify", "--quiet", &format!("refs/remotes/{remote}")]).is_some() {
        remote
    } else {
        main.to_string()
    }
}

/// One `git rev-list` per non-main branch.
pub fn collect(repo: &Path, branches: &[Branch], trunk: &str, tips: HashMap<String, String>) -> GitView {
    let max = format!("--max-count={MAX_COMMITS}");
    let order: HashMap<String, Vec<String>> = branches
        .iter()
        .filter(|b| !b.is_main && tips.contains_key(&b.name))
        .map(|b| {
            let range = git(repo, &["rev-list", &max, &b.name, &format!("^{trunk}")]).unwrap_or_default();
            (b.name.clone(), range.lines().map(str::to_owned).collect())
        })
        .collect();
    let unmerged = order.iter().map(|(b, c)| (b.clone(), c.iter().cloned().collect())).collect();
    GitView { tips, unmerged, order }
}

/// `origin/*` branch tips in one call: commit → branch name (without
/// `origin/`; the alphabetically first when several share a commit).
pub fn remote_tips(repo: &Path) -> HashMap<String, String> {
    let out = git(repo, &["for-each-ref", "--format=%(refname:lstrip=3) %(objectname)", "refs/remotes/origin"]).unwrap_or_default();
    parse_remote_tips(&out)
}

fn parse_remote_tips(out: &str) -> HashMap<String, String> {
    let mut tips: HashMap<String, String> = HashMap::new();
    for (name, sha) in out.lines().filter_map(|l| l.split_once(' ')) {
        if name == "HEAD" || name.is_empty() {
            continue;
        }
        let e = tips.entry(sha.to_string()).or_insert_with(|| name.to_string());
        if name < e.as_str() {
            *e = name.to_string();
        }
    }
    tips
}

/// The branch `branch` was built on when that isn't the trunk: its open PR's
/// base (`pr_base`), else the newest of its unmerged commits that is the tip
/// of another `origin/*` branch (`remote`, from [`remote_tips`]). `None` for
/// the trunk (`main`) or when nothing says. Pure.
pub fn base_branch(branch: &str, main: &str, pr_base: Option<&str>, git: &GitView, remote: &HashMap<String, String>) -> Option<String> {
    if let Some(base) = pr_base {
        return (base != main && base != branch).then(|| base.to_string());
    }
    let commits = git.order.get(branch)?;
    let tip = git.tips.get(branch);
    commits
        .iter()
        .filter(|c| Some(*c) != tip)
        .find_map(|c| remote.get(c).filter(|n| n.as_str() != branch && n.as_str() != main))
        .cloned()
}

/// child worktree id → parent worktree id. `pr_base` maps worktree id to its
/// PR's base branch. Pure, so the rules are unit-tested without git.
pub fn infer_parents(branches: &[Branch], git: &GitView, pr_base: &HashMap<String, String>) -> HashMap<String, String> {
    let by_name: HashMap<&str, &Branch> = branches.iter().map(|b| (b.name.as_str(), b)).collect();
    let mut parents = HashMap::new();
    for b in branches.iter().filter(|b| !b.is_main) {
        // 1. PR base is another (non-main) worktree's branch.
        if let Some(parent) = pr_base.get(&b.worktree_id).and_then(|base| by_name.get(base.as_str()))
            && !parent.is_main
            && parent.worktree_id != b.worktree_id
        {
            parents.insert(b.worktree_id.clone(), parent.worktree_id.clone());
            continue;
        }
        // 2. Closest other branch whose tip is in this branch's unmerged commits.
        let (Some(mine), Some(my_tip)) = (git.unmerged.get(&b.name), git.tips.get(&b.name)) else { continue };
        let parent = branches
            .iter()
            .filter(|a| !a.is_main && a.name != b.name)
            .filter_map(|a| {
                let tip = git.tips.get(&a.name)?;
                // Same commit: neither is stacked on the other yet.
                (tip != my_tip && mine.contains(tip)).then(|| (a, git.unmerged.get(&a.name).map_or(0, HashSet::len)))
            })
            .max_by_key(|(_, depth)| *depth)
            .map(|(a, _)| a.worktree_id.clone());
        if let Some(p) = parent {
            parents.insert(b.worktree_id.clone(), p);
        }
    }
    break_cycles(&mut parents);
    parents
}

/// PR bases can disagree with git; never let that produce a loop.
fn break_cycles(parents: &mut HashMap<String, String>) {
    let ids: Vec<String> = parents.keys().cloned().collect();
    for start in ids {
        let mut seen = HashSet::new();
        let mut cur = start.clone();
        while let Some(next) = parents.get(&cur) {
            if !seen.insert(cur.clone()) {
                parents.remove(&start);
                break;
            }
            cur = next.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn br(id: &str, name: &str) -> Branch {
        Branch { worktree_id: id.into(), name: name.into(), is_main: false }
    }

    fn view(tips: &[(&str, &str)], unmerged: &[(&str, &[&str])]) -> GitView {
        GitView {
            tips: tips.iter().map(|(b, s)| (b.to_string(), s.to_string())).collect(),
            unmerged: unmerged.iter().map(|(b, c)| (b.to_string(), c.iter().map(|s| s.to_string()).collect())).collect(),
            order: unmerged.iter().map(|(b, c)| (b.to_string(), c.iter().map(|s| s.to_string()).collect())).collect(),
        }
    }

    #[test]
    fn remote_tips_skip_head_and_keep_the_first_name_per_commit() {
        let t = parse_remote_tips("HEAD 1\nmain 1\nfeature/b 2\nfeature/a 2\n");
        assert_eq!(t.get("1").map(String::as_str), Some("main"));
        assert_eq!(t.get("2").map(String::as_str), Some("feature/a"));
    }

    #[test]
    fn base_branch_prefers_the_pr_then_the_closest_remote_branch() {
        // c(3) ← b(2) ← a(1) ← main; b and a are on origin, c is pushed too.
        let g = view(&[("c", "3")], &[("c", &["3", "2", "1"])]);
        let remote = HashMap::from([
            ("3".to_string(), "c".to_string()),
            ("2".to_string(), "b".to_string()),
            ("1".to_string(), "a".to_string()),
        ]);
        assert_eq!(base_branch("c", "main", None, &g, &remote).as_deref(), Some("b"), "closest, not own tip");
        assert_eq!(base_branch("c", "main", Some("x"), &g, &remote).as_deref(), Some("x"), "PR base wins");
        assert_eq!(base_branch("c", "main", Some("main"), &g, &remote), None, "on the trunk");
        assert_eq!(base_branch("c", "main", None, &g, &HashMap::new()), None, "nothing on origin");
        assert_eq!(base_branch("unknown", "main", None, &g, &remote), None);
    }

    #[test]
    fn picks_the_closest_ancestor_branch() {
        // main ← a(1) ← b(2) ← c(3)
        let branches = [br("A", "a"), br("B", "b"), br("C", "c"), br("D", "d")];
        let g = view(
            &[("a", "1"), ("b", "2"), ("c", "3"), ("d", "9")],
            &[("a", &["1"]), ("b", &["2", "1"]), ("c", &["3", "2", "1"]), ("d", &["9"])],
        );
        let p = infer_parents(&branches, &g, &HashMap::new());
        assert_eq!(p.get("B").map(String::as_str), Some("A"));
        assert_eq!(p.get("C").map(String::as_str), Some("B"), "c sits on b, not a");
        assert!(!p.contains_key("A") && !p.contains_key("D"));
    }

    #[test]
    fn pr_base_wins_and_ignores_main() {
        let mut main = br("M", "main");
        main.is_main = true;
        let branches = [main, br("A", "a"), br("B", "b")];
        let g = view(&[("a", "1"), ("b", "2")], &[("a", &["1"]), ("b", &["2"])]);
        let prs = HashMap::from([("B".to_string(), "a".to_string()), ("A".to_string(), "main".to_string())]);
        let p = infer_parents(&branches, &g, &prs);
        assert_eq!(p.get("B").map(String::as_str), Some("A"));
        assert!(!p.contains_key("A"));
    }

    #[test]
    fn same_tip_is_not_a_stack_and_cycles_are_broken() {
        let branches = [br("A", "a"), br("B", "b")];
        let g = view(&[("a", "1"), ("b", "1")], &[("a", &["1"]), ("b", &["1"])]);
        assert!(infer_parents(&branches, &g, &HashMap::new()).is_empty());
        let prs = HashMap::from([("A".to_string(), "b".to_string()), ("B".to_string(), "a".to_string())]);
        let p = infer_parents(&branches, &view(&[], &[]), &prs);
        assert!(p.len() < 2, "a ↔ b loop must be broken: {p:?}");
    }

    #[test]
    fn real_git_stack() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        let g = |args: &[&str]| {
            assert!(Command::new("git")
                .args(["-c", "user.email=t@t", "-c", "user.name=t", "-c", "init.defaultBranch=main"])
                .args(args)
                .current_dir(r)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .output()
                .unwrap()
                .status
                .success());
        };
        g(&["init", "-q"]);
        g(&["commit", "-q", "--allow-empty", "-m", "root"]);
        g(&["checkout", "-q", "-b", "base-feature"]);
        g(&["commit", "-q", "--allow-empty", "-m", "a1"]);
        g(&["checkout", "-q", "-b", "stacked"]);
        g(&["commit", "-q", "--allow-empty", "-m", "b1"]);
        g(&["checkout", "-q", "main"]);
        g(&["checkout", "-q", "-b", "unrelated"]);
        g(&["commit", "-q", "--allow-empty", "-m", "u1"]);
        let mut main = br("M", "main");
        main.is_main = true;
        let branches = [main, br("A", "base-feature"), br("B", "stacked"), br("U", "unrelated")];
        let trunk = trunk_ref(r, "main");
        assert_eq!(trunk, "main");
        let gv = collect(r, &branches, &trunk, branch_tips(r));
        let p = infer_parents(&branches, &gv, &HashMap::new());
        assert_eq!(p, HashMap::from([("B".to_string(), "A".to_string())]));
        // Only `stacked` is a worktree now; its base is on origin (faked as a ref).
        g(&["update-ref", "refs/remotes/origin/base-feature", "base-feature"]);
        g(&["update-ref", "refs/remotes/origin/stacked", "stacked"]);
        let branches = [br("B", "stacked")];
        let gv = collect(r, &branches, &trunk, branch_tips(r));
        assert_eq!(base_branch("stacked", "main", None, &gv, &remote_tips(r)).as_deref(), Some("base-feature"));
    }
}
