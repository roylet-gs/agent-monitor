//! Background worktree jobs (create / delete / live branch check). Each runs
//! on its own thread with its own DB connection and reports back via Msg.

use std::path::Path;
use std::sync::mpsc::Sender;

use am_core::model::{Repository, WorktreeView};
use am_core::settings::Settings;
use am_core::worktrees::{self, CreateRequest, DeleteOptions};
use am_core::{db, info};

use crate::launcher::Msg;

pub fn check_branch(repo_path: String, branch: String, tx: Sender<Msg>) {
    std::thread::spawn(move || {
        let r = worktrees::check_branch(Path::new(&repo_path), &branch);
        am_core::debug!("tui", "Live branch check {branch}: local={} remote={:?}", r.local, r.remote);
        let _ = tx.send(Msg::BranchChecked(branch, r));
    });
}

pub fn create(settings: Settings, repo: Repository, req: CreateRequest, tx: Sender<Msg>) {
    std::thread::spawn(move || {
        let result = db::open()
            .map_err(|e| worktrees::CreateError::Db(e.to_string()))
            .and_then(|conn| worktrees::create_worktree(&conn, &settings, &repo, &req));
        if let Ok(w) = &result {
            info!("tui", "Created worktree {} at {}", w.branch, w.path);
        }
        let _ = tx.send(Msg::Created(Box::new(repo), Box::new(req), result));
    });
}

pub fn delete(view: WorktreeView, delete_branch: bool, tx: Sender<Msg>) {
    std::thread::spawn(move || {
        // The user confirmed after seeing the dirty/unpushed warnings, so
        // remove like the TS dashboard does (force; `git branch -D`).
        let opts = DeleteOptions { delete_branch, delete_remote: false, force: true };
        let result = db::open().map_err(|e| e.to_string()).and_then(|conn| worktrees::delete_worktree(&conn, &view.worktree, &opts));
        let label = view.worktree.display_name().to_string();
        if let Err(e) = &result {
            am_core::warn!("tui", "Delete of {} failed: {e}", view.worktree.branch);
        }
        let _ = tx.send(Msg::Deleted(view.worktree.id.clone(), label, result.map(|r| r.warnings)));
    });
}

pub fn add_repo(path: String, tx: Sender<Msg>) {
    std::thread::spawn(move || {
        let expanded = match path.strip_prefix('~') {
            Some(rest) => format!("{}{rest}", am_core::paths::home_dir().display()),
            None => path.clone(),
        };
        let result = db::open()
            .map_err(|e| e.to_string())
            .and_then(|conn| worktrees::add_repository(&conn, Path::new(expanded.trim_end_matches('/'))).map_err(|e| e.to_string()));
        let _ = tx.send(Msg::RepoAdded(path, result.map(|r| r.name)));
    });
}

/// The app's own repo, for feature requests (same as the TS fallback).
const OWN_REPO: &str = "roylet-gs/agent-monitor";

/// `gh issue create` on the app's repo with an environment footer
/// (port of `createFeatureRequest`).
pub fn suggest(title: String, body: String, tx: Sender<Msg>) {
    std::thread::spawn(move || {
        let full = format!(
            "{body}\n\n---\n_Submitted from agent-monitor_\n- am version: {} (native)\n- platform: {}",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS
        );
        let out = std::process::Command::new("gh")
            .args(["issue", "create", "--repo", OWN_REPO, "--title", &title, "--body", &full])
            .stdin(std::process::Stdio::null())
            .output();
        let result = match out {
            Ok(o) if o.status.success() => Ok(String::from_utf8_lossy(&o.stdout).trim().to_string()),
            Ok(o) => Err(String::from_utf8_lossy(&o.stderr).lines().next().unwrap_or("gh failed").to_string()),
            Err(e) => Err(format!("Couldn't run gh: {e}")),
        };
        if let Ok(url) = &result {
            info!("tui", "Feature request created: {url}");
        }
        let _ = tx.send(match result {
            Ok(url) => Msg::Popup(format!("✓ Feature request filed — thanks!\n\n{url}")),
            Err(e) => Msg::Popup(format!("✗ {e}")),
        });
    });
}
