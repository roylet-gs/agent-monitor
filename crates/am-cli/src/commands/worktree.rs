//! `am worktree list|info|open` + the `ls` / `open` aliases
//! (src/commands/worktree/{list,info,open}.ts).
//!
//! `create`, `delete`, `sync` and `new` are stubbed in main.rs until the shared
//! `am_core::worktrees` module lands.

use std::path::Path;
use std::process::{Command, Stdio};

use am_core::integrations::github::pr_status_label;
use am_core::integrations::{FetchResult, Integrations, RepoBranches};
use am_core::launch::{self, IdeChoice};
use am_core::model::{CommitInfo, GitStatus, Repository, WorktreeView};
use am_core::settings::{self, Settings};
use am_core::{gitinfo, grouping, info, timefmt};
use serde_json::{Value, json};

use crate::output::{self, Column, col};
use crate::resolve::{resolve_repo, resolve_worktree};
use crate::{fail, store};

/// git's `%cr` ("3 hours ago", "2 years, 1 month ago"), which the TS
/// commands printed; `CommitInfo` carries a timestamp instead.
pub fn git_relative(diff: i64) -> String {
    fn n(v: i64, unit: &str) -> String {
        if v == 1 { format!("1 {unit}") } else { format!("{v} {unit}s") }
    }
    if diff < 0 {
        return "in the future".into();
    }
    if diff < 90 {
        return format!("{} ago", n(diff, "second"));
    }
    let minutes = (diff + 30) / 60;
    if minutes < 90 {
        return format!("{} ago", n(minutes, "minute"));
    }
    let hours = (minutes + 30) / 60;
    if hours < 36 {
        return format!("{} ago", n(hours, "hour"));
    }
    let days = (hours + 12) / 24;
    if days < 14 {
        return format!("{} ago", n(days, "day"));
    }
    if days < 70 {
        return format!("{} ago", n((days + 3) / 7, "week"));
    }
    if days < 365 {
        return format!("{} ago", n((days + 15) / 30, "month"));
    }
    if days < 1825 {
        let total_months = (days * 12 * 2 + 365) / (365 * 2);
        let (years, months) = (total_months / 12, total_months % 12);
        return if months > 0 {
            format!("{}, {} ago", n(years, "year"), n(months, "month"))
        } else {
            format!("{} ago", n(years, "year"))
        };
    }
    format!("{} ago", n((days + 183) / 365, "year"))
}

fn commit_json(c: &CommitInfo) -> Value {
    json!({
        "hash": c.hash,
        "message": c.message,
        "relative_time": git_relative(timefmt::now_unix() - c.timestamp),
        "timestamp": c.timestamp,
    })
}

fn git_json(g: &GitStatus) -> Value {
    json!({ "ahead": g.ahead, "behind": g.behind, "dirty": g.dirty })
}

/// Refresh the shared PR/Linear cache (batched: one `gh` call per repo, one
/// Linear request per 25 branches) and return it. Every tracked worktree is
/// passed because a refresh prunes cache keys it isn't given. A failed fetch
/// keeps the cached value, so offline listings still show the last known data.
fn refreshed_integrations(conn: &rusqlite::Connection, s: &Settings) -> Integrations {
    let mut integ = Integrations::new();
    integ.hydrate(conn);
    let worktrees = store::worktrees(conn);
    let all: Vec<RepoBranches> = store::repositories(conn)
        .into_iter()
        .map(|r| {
            let branches = worktrees.iter().filter(|w| w.repo_id == r.id).map(|w| w.branch.clone()).collect();
            RepoBranches::new(r.id, r.path, branches)
        })
        .collect();
    if s.gh_pr_status {
        integ.refresh_github(conn, &all, false);
    }
    if s.linear_enabled && !s.linear_api_key.is_empty() {
        integ.refresh_linear(conn, &s.linear_api_key, &all, false);
    }
    integ
}

pub fn list(repo_path: Option<&str>, as_json: bool) {
    let conn = store::open();
    let repos: Vec<Repository> = match repo_path {
        Some(p) => vec![resolve_repo(&conn, Some(p)).repository()],
        None => {
            let all = store::repositories(&conn);
            if all.is_empty() {
                fail("No repositories tracked. Run: am repo add <path>");
            }
            all.iter().map(|r| r.repository()).collect()
        }
    };
    let s = settings::load();
    let multi = repos.len() > 1;
    let agents = am_core::db::agent_states(&conn).unwrap_or_default();
    let integ = refreshed_integrations(&conn, &s);

    let views: Vec<WorktreeView> = store::worktrees(&conn)
        .into_iter()
        .filter(|w| repos.iter().any(|r| r.id == w.repo_id))
        // Main/master rows are hidden only in the all-repos listing.
        .filter(|w| !(multi && repo_path.is_none() && ((s.hide_main_branch && w.branch == "main") || w.branch == "master")))
        .filter(|w| Path::new(&w.path).exists())
        .map(|w| {
            let path = Path::new(&w.path);
            WorktreeView {
                agent: agents.get(&w.id).cloned(),
                git: gitinfo::status(path),
                last_commit: gitinfo::last_commit(path),
                pr: s.gh_pr_status.then(|| integ.cached_pr(&w.repo_id, &w.branch).cloned()).flatten(),
                linear: (s.linear_enabled && !s.linear_api_key.is_empty())
                    .then(|| integ.cached_linear(&w.repo_id, &w.branch).cloned())
                    .flatten(),
                worktree: w,
                has_terminal: false,
                open_ide: None,
                running_processes: Vec::new(),
                context: None,
                sessions: Vec::new(),
            }
        })
        .collect();
    // Same order as the dashboard (the `worktreeSort` setting); no view filters.
    let groups = grouping::build_groups(&repos, views, &s);

    let mut json_rows = Vec::new();
    let mut rows = Vec::new();
    for g in &groups {
        for v in &g.worktrees {
            let w = &v.worktree;
            let agent = v.agent.as_ref().map(|a| a.status.as_str());
            let git = v.git.unwrap_or_default();
            let pr_label = v.pr.as_ref().map(|p| pr_status_label(p).0.to_string()).unwrap_or_default();
            let linear_label =
                v.linear.as_ref().map(|l| format!("{} {}", l.identifier, l.state.name)).unwrap_or_default();
            if as_json {
                json_rows.push(json!({
                    "repo": g.repo.name,
                    "repoPath": g.repo.path,
                    "branch": w.branch,
                    "path": w.path,
                    "agent": agent.unwrap_or("unknown"),
                    "agentCli": v.agent.as_ref().map(|a| a.agent.id()),
                    "git": git_json(&git),
                    "lastCommit": v.last_commit.as_ref().map(commit_json),
                    "pr": (!pr_label.is_empty()).then_some(&pr_label),
                    "prUrl": v.pr.as_ref().map(|p| &p.url),
                    "linear": (!linear_label.is_empty()).then_some(&linear_label),
                }));
                continue;
            }
            let git_info = [
                (git.dirty > 0).then(|| format!("{}M", git.dirty)),
                (git.ahead > 0).then(|| format!("↑{}", git.ahead)),
                (git.behind > 0).then(|| format!("↓{}", git.behind)),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" ");
            let commit = v
                .last_commit
                .as_ref()
                .map(|c| format!("{} {}", c.hash, git_relative(timefmt::now_unix() - c.timestamp)))
                .unwrap_or_default();
            rows.push(vec![
                ("repo", if multi { g.repo.name.clone() } else { String::new() }),
                ("branch", w.branch.clone()),
                ("agent", agent.unwrap_or("—").to_string()),
                ("git", if git_info.is_empty() { "clean".into() } else { git_info }),
                ("commit", commit),
                ("pr", pr_label),
                ("linear", linear_label),
            ]);
        }
    }

    if as_json {
        output::json(&Value::Array(json_rows));
        return;
    }
    let has = |key: &str| rows.iter().any(|r| r.iter().any(|(k, v)| *k == key && !v.is_empty()));
    let mut columns: Vec<Column> = Vec::new();
    if multi {
        columns.push(col("repo", "Repo"));
    }
    columns.extend([col("branch", "Branch"), col("agent", "Agent"), col("git", "Git"), col("commit", "Last Commit")]);
    if has("pr") {
        columns.push(col("pr", "PR"));
    }
    if has("linear") {
        columns.push(col("linear", "Linear"));
    }
    output::table(&rows, &columns);
}

pub fn info(target: &str, repo_path: Option<&str>, as_json: bool) {
    let conn = store::open();
    let repo = resolve_repo(&conn, repo_path);
    let wt = resolve_worktree(&conn, target, Some(&repo.id));
    let s = settings::load();
    let agent = store::agent_status(&conn, &wt.id);
    let path = Path::new(&wt.path);
    let exists = path.exists();
    let git = if exists { gitinfo::status(path) } else { None };
    let commit = if exists { gitinfo::last_commit(path) } else { None };

    // Live one-shot lookups; on failure fall back to the shared cache.
    let mut integ = Integrations::new();
    integ.hydrate(&conn);
    let pr = s.gh_pr_status.then(|| match integ.fetch_pr_now(Path::new(&repo.path), &wt.branch) {
        FetchResult::Ok(p) => p,
        FetchResult::Err(_) => integ.cached_pr(&repo.id, &wt.branch).cloned(),
    });
    let linear = (s.linear_enabled && !s.linear_api_key.is_empty()).then(|| {
        match integ.fetch_linear_now(&s.linear_api_key, &wt.branch) {
            FetchResult::Ok(l) => l,
            FetchResult::Err(_) => integ.cached_linear(&repo.id, &wt.branch).cloned(),
        }
    });
    let (pr, linear) = (pr.flatten(), linear.flatten());

    if as_json {
        output::json(&json!({
            "branch": wt.branch,
            "path": wt.path,
            "exists": exists,
            "repo": repo.name,
            "repoPath": repo.path,
            "agent": agent.as_ref().map(|a| json!({ "status": a.status, "sessionId": a.session_id, "updatedAt": a.updated_at, "cli": a.agent.id() })),
            "git": git.as_ref().map(git_json),
            "lastCommit": commit.as_ref().map(commit_json),
            "pr": pr,
            "linear": linear,
        }));
        return;
    }

    let mut pairs: Vec<(String, String)> = vec![
        ("Branch:".into(), wt.branch.clone()),
        ("Path:".into(), wt.path.clone()),
        ("Exists:".into(), if exists { "yes".into() } else { "NO — worktree directory missing".into() }),
        ("Repo:".into(), format!("{} ({})", repo.name, repo.path)),
        ("Agent:".into(), agent.as_ref().map(|a| format!("{} ({})", a.status, a.agent.provider().name())).unwrap_or_else(|| "unknown".into())),
    ];
    if let Some(sid) = agent.as_ref().and_then(|a| a.session_id.clone()).filter(|s| !s.is_empty()) {
        pairs.push(("Session:".into(), sid));
    }
    if let Some(a) = agent.as_ref().filter(|a| !a.updated_at.is_empty()) {
        pairs.push(("Updated:".into(), a.updated_at.clone()));
    }
    if let Some(g) = git {
        let parts: Vec<String> = [
            Some(if g.dirty > 0 { format!("{} modified", g.dirty) } else { "clean".into() }),
            (g.ahead > 0).then(|| format!("{} ahead", g.ahead)),
            (g.behind > 0).then(|| format!("{} behind", g.behind)),
        ]
        .into_iter()
        .flatten()
        .collect();
        pairs.push(("Git:".into(), parts.join(", ")));
    }
    if let Some(c) = &commit {
        pairs.push((
            "Commit:".into(),
            format!("{} {} ({})", c.hash, c.message, git_relative(timefmt::now_unix() - c.timestamp)),
        ));
    }
    if let Some(p) = &pr {
        pairs.push(("PR:".into(), format!("#{} {}", p.number, p.title)));
        pairs.push(("PR Status:".into(), pr_status_label(p).0.into()));
        pairs.push(("PR URL:".into(), p.url.clone()));
    }
    if let Some(l) = &linear {
        pairs.push(("Linear:".into(), format!("{} {}", l.identifier, l.title)));
        pairs.push(("Linear Status:".into(), l.state.name.clone()));
        pairs.push(("Linear URL:".into(), l.url.clone()));
    }
    if let Some(summary) = agent.as_ref().and_then(|a| a.transcript_summary.as_deref()).filter(|s| !s.is_empty()) {
        pairs.push(("Summary:".into(), summary.chars().take(200).collect()));
    }
    output::key_value(&pairs);
}

fn git_ok(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).current_dir(dir).stderr(Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Port of `ensureBranchForOpen`: the main checkout is switched to the row's
/// branch (when clean) before opening; linked worktrees are opened as-is.
fn ensure_branch_for_open(dir: &Path, expected: &str, is_main: bool) -> Result<bool, String> {
    if !is_main {
        return Ok(false);
    }
    let current = || {
        git_ok(dir, &["symbolic-ref", "--short", "HEAD"])
            .or_else(|| git_ok(dir, &["rev-parse", "--abbrev-ref", "HEAD"]))
            .unwrap_or_default()
    };
    let default = if git_ok(dir, &["rev-parse", "--verify", "main"]).is_some() {
        "main".to_string()
    } else if git_ok(dir, &["rev-parse", "--verify", "master"]).is_some() {
        "master".to_string()
    } else {
        current()
    };
    if expected == default || current() == expected {
        return Ok(false);
    }
    let exists = git_ok(dir, &["rev-parse", "--verify", &format!("refs/heads/{expected}")]).is_some()
        || git_ok(dir, &["rev-parse", "--verify", &format!("refs/remotes/origin/{expected}")]).is_some();
    if !exists {
        return Err(format!("Branch \"{expected}\" does not exist."));
    }
    let dirty = gitinfo::status(dir).map(|g| g.dirty).unwrap_or(0);
    if dirty > 0 {
        return Err(format!(
            "Cannot switch to branch {expected}: {dirty} uncommitted change{}. Stash or commit first.",
            if dirty == 1 { "" } else { "s" }
        ));
    }
    let out = Command::new("git")
        .args(["checkout", expected])
        .current_dir(dir)
        .output()
        .map_err(|e| format!("git checkout failed: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    info!("git", "Checked out branch {expected} in {}", dir.display());
    Ok(true)
}

pub fn open(target: &str, repo_path: Option<&str>) {
    let conn = store::open();
    let repo = resolve_repo(&conn, repo_path);
    let wt = resolve_worktree(&conn, target, Some(&repo.id));
    let s = settings::load();
    let path = Path::new(&wt.path);
    match ensure_branch_for_open(path, &wt.branch, wt.is_main) {
        Err(e) => fail(&e),
        Ok(true) => println!("Switched to branch {}", wt.branch),
        Ok(false) => {}
    }
    if let Err(e) = launch::open_in_editor(path, IdeChoice::from_setting(&s.ide)) {
        fail(&e);
    }
    println!("Opened {} in {}", wt.branch, s.ide);
}

#[cfg(test)]
mod tests {
    use super::git_relative;

    #[test]
    fn relative_times_match_git() {
        assert_eq!(git_relative(5), "5 seconds ago");
        assert_eq!(git_relative(60 * 60 * 3), "3 hours ago");
        assert_eq!(git_relative(86_400), "24 hours ago");
        assert_eq!(git_relative(86_400 * 2), "2 days ago");
        assert_eq!(git_relative(86_400 * 20), "3 weeks ago");
        assert_eq!(git_relative(86_400 * 400), "1 year, 1 month ago");
        assert_eq!(git_relative(86_400 * 3650), "10 years ago");
    }
}
