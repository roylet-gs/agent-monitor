//! `am pr [target]` / `am pr open [target]` (src/commands/pr.ts).

use std::path::Path;

use am_core::integrations::{FetchResult, Integrations};
use am_core::integrations::github::pr_status_label;
use am_core::model::{ChecksStatus, PrInfo};
use am_core::warn;

use crate::resolve::resolve_repo;
use crate::{fail, output, store};

fn pr_for_target(target: Option<&str>, repo_path: Option<&str>) -> (Option<PrInfo>, String) {
    let conn = store::open();
    let repo = resolve_repo(&conn, repo_path);
    let branch = super::target_branch(&conn, target, &repo.id);
    let pr = match Integrations::new().fetch_pr_now(Path::new(&repo.path), &branch) {
        FetchResult::Ok(pr) => pr,
        // The TS command's lossy `fetchPrInfo` treated failure as "no PR".
        FetchResult::Err(e) => {
            warn!("cli", "PR lookup for {branch} failed: {e}");
            None
        }
    };
    (pr, branch)
}

pub fn checks_label(c: ChecksStatus) -> &'static str {
    match c {
        ChecksStatus::Pending => "pending",
        ChecksStatus::Passing => "passing",
        ChecksStatus::Failing => "failing",
        ChecksStatus::None => "none",
    }
}

pub fn show(target: Option<&str>, repo_path: Option<&str>, json: bool) {
    let (pr, branch) = pr_for_target(target, repo_path);
    let Some(pr) = pr else {
        println!("No PR found for branch: {branch}");
        return;
    };
    if json {
        output::json(&serde_json::to_value(&pr).unwrap_or_default());
        return;
    }
    let (label, _) = pr_status_label(&pr);
    output::key_value(&[
        ("PR:".into(), format!("#{} {}", pr.number, pr.title)),
        ("Status:".into(), label.into()),
        ("State:".into(), pr.state.clone()),
        ("Draft:".into(), if pr.is_draft { "yes" } else { "no" }.into()),
        ("Checks:".into(), checks_label(pr.checks_status).into()),
        ("URL:".into(), pr.url.clone()),
    ]);
}

pub fn open(target: Option<&str>, repo_path: Option<&str>) {
    let (pr, branch) = pr_for_target(target, repo_path);
    let Some(pr) = pr else {
        println!("No PR found for branch: {branch}");
        return;
    };
    if let Err(e) = super::open_url(&pr.url) {
        fail(&format!("Failed to open {}: {e}", pr.url));
    }
    println!("Opened: {}", pr.url);
}
