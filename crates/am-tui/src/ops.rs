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
        let _ = tx.send(Msg::Deleted(label, result.map(|r| r.warnings)));
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
