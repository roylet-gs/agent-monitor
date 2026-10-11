//! `am linear [target]` / `am linear open [target]` (src/commands/linear.ts).

use am_core::integrations::{FetchResult, Integrations};
use am_core::model::LinearInfo;
use am_core::settings::{self, Settings};
use am_core::warn;

use crate::resolve::resolve_repo;
use crate::{fail, output, store};

fn linear_for_target(s: &Settings, target: Option<&str>, repo_path: Option<&str>) -> (Option<LinearInfo>, String) {
    if !s.linear_enabled || s.linear_api_key.is_empty() {
        fail("Linear integration not configured. Set linearEnabled=true and linearApiKey in settings.");
    }
    let conn = store::open();
    let repo = resolve_repo(&conn, repo_path);
    let branch = super::target_branch(&conn, target, &repo.id);
    let info = match Integrations::new().fetch_linear_now(&s.linear_api_key, &branch) {
        FetchResult::Ok(i) => i,
        FetchResult::Err(e) => {
            warn!("cli", "Linear lookup for {branch} failed: {e}");
            None
        }
    };
    (info, branch)
}

pub fn show(target: Option<&str>, repo_path: Option<&str>, json: bool) {
    let s = settings::load();
    let (info, branch) = linear_for_target(&s, target, repo_path);
    let Some(info) = info else {
        println!("No Linear ticket found for branch: {branch}");
        return;
    };
    if json {
        output::json(&serde_json::to_value(&info).unwrap_or_default());
        return;
    }
    output::key_value(&[
        ("Ticket:".into(), format!("{} {}", info.identifier, info.title)),
        ("Status:".into(), info.state.name.clone()),
        ("Priority:".into(), info.priority_label.clone()),
        ("Assignee:".into(), info.assignee.clone().unwrap_or_else(|| "unassigned".into())),
        ("URL:".into(), info.url.clone()),
    ]);
}

pub fn open(target: Option<&str>, repo_path: Option<&str>) {
    let s = settings::load();
    let (info, branch) = linear_for_target(&s, target, repo_path);
    let Some(info) = info else {
        println!("No Linear ticket found for branch: {branch}");
        return;
    };
    let url =
        if s.linear_use_desktop_app { info.url.replacen("https://linear.app", "linear://", 1) } else { info.url.clone() };
    if let Err(e) = super::open_url(&url) {
        fail(&format!("Failed to open {url}: {e}"));
    }
    println!("Opened: {url}");
}
