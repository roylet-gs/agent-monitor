//! `worktree create|delete|sync` and `new` — ports of
//! src/commands/worktree/{create,delete,sync}.ts on top of am_core::worktrees.

use std::path::Path;

use am_core::worktrees::{self, CreateError, CreateMode, CreateRequest, DeleteKind, DeleteOptions};
use serde_json::json;

use crate::resolve::{detect_repo, resolve_repo, resolve_worktree};
use crate::{CreateArgs, fail, hooks_installer, output, store};

pub fn create(a: &CreateArgs) {
    if a.reuse && a.track {
        fail("--reuse and --track are mutually exclusive.");
    }
    if a.track && a.no_track {
        fail("--track and --no-track are mutually exclusive.");
    }
    let conn = store::open();
    let repo = resolve_repo(&conn, a.repo.repo.as_deref()).repository();
    let settings = am_core::settings::load();
    let mode = if a.reuse {
        CreateMode::ReuseLocal
    } else if a.track {
        CreateMode::TrackRemote
    } else if a.no_track {
        CreateMode::Disconnected
    } else {
        CreateMode::Fresh
    };
    let req = CreateRequest { branch: a.branch.clone(), base_branch: a.base.clone(), custom_name: a.name.clone(), mode };
    let w = match worktrees::create_worktree(&conn, &settings, &repo, &req) {
        Ok(w) => w,
        Err(CreateError::BranchExists { local, remote, .. }) => {
            let where_ = match (local, remote) {
                (true, true) => "locally and on origin",
                (_, true) => "on origin",
                _ => "locally",
            };
            let hint = if remote {
                "Use --track to pull remote, --no-track to create disconnected, or --reuse for the existing local branch."
            } else {
                "Use --reuse to attach to the existing branch."
            };
            fail(&format!("Branch \"{}\" already exists {where_}. {hint}", a.branch));
        }
        Err(CreateError::Invalid(m)) if a.track => fail(&format!("Cannot --track: {m}")),
        Err(e) => fail(&e.to_string()),
    };

    for kind in am_core::agents::detected(&am_core::settings::load()) {
        if !hooks_installer::is_hooks_installed(kind) && hooks_installer::install_hooks(kind).is_ok() {
            println!("{} hooks installed automatically.", kind.provider().short_name());
        }
    }

    let base = a.base.clone().unwrap_or_else(|| worktrees::main_branch(Path::new(&repo.path)).unwrap_or_else(|_| "main".into()));
    let mode_name = match mode {
        CreateMode::ReuseLocal => "reuse",
        CreateMode::TrackRemote => "track",
        CreateMode::Disconnected => "no-track",
        _ => "fresh",
    };
    if a.json.json {
        output::json(&json!({ "branch": w.branch, "path": w.path, "repo": repo.name, "mode": mode_name }));
    } else {
        println!("Created worktree: {}", w.path);
        let desc = match mode {
            CreateMode::TrackRemote => format!("{} (tracking origin/{})", w.branch, w.branch),
            CreateMode::Disconnected => format!("{} (new, not tracking remote — based on {base})", w.branch),
            CreateMode::ReuseLocal => format!("{} (reused existing)", w.branch),
            _ => format!("{} (based on {base})", w.branch),
        };
        println!("Branch: {desc}");
    }
}

pub fn delete(target: &str, repo: Option<&str>, force: bool, delete_branch: bool, delete_remote: bool) {
    let conn = store::open();
    let repo = resolve_repo(&conn, repo);
    let wt = resolve_worktree(&conn, target, Some(&repo.id));
    let opts = DeleteOptions { delete_branch, delete_remote, force };
    match worktrees::delete_worktree(&conn, &wt, &opts) {
        Ok(report) => {
            for w in &report.warnings {
                eprintln!("{w}");
            }
            if report.branch_deleted {
                println!("Deleted local branch: {}", wt.branch);
            }
            if report.remote_deleted {
                println!("Deleted remote branch: origin/{}", wt.branch);
            }
            match report.kind {
                DeleteKind::Worktree => println!("Deleted worktree: {}", report.path),
                DeleteKind::Stale => println!("Deleted stale entry: {}", wt.branch),
                DeleteKind::BranchOnly => println!("Deleted branch-only entry: {}", wt.branch),
            }
        }
        Err(e) if !force => fail(&format!("Failed to delete worktree. Use --force to override.\n{e}")),
        Err(e) => fail(&e),
    }
}

pub fn sync(repo: Option<&str>) {
    let conn = store::open();
    let repos = match repo {
        Some(p) => vec![resolve_repo(&conn, Some(p))],
        None => {
            let cwd = std::env::current_dir().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
            match detect_repo(&conn, &cwd) {
                Some(r) => vec![r],
                None => store::repositories(&conn),
            }
        }
    };
    if repos.is_empty() {
        fail("No repositories tracked. Run: am repo add <path>");
    }
    for r in repos {
        match worktrees::sync_repo(&conn, &r.repository()) {
            Ok(_) => println!("Synced: {} ({})", r.name, r.path),
            Err(e) => eprintln!("Sync failed for {}: {e}", r.name),
        }
    }
}

/// After `repo add`: make the repo's worktrees show up immediately.
pub fn sync_new_repo(conn: &rusqlite::Connection, repo: &am_core::model::Repository) {
    if let Err(e) = worktrees::sync_repo(conn, repo) {
        am_core::warn!("cli", "Initial sync of {} failed: {e}", repo.name);
    }
}
