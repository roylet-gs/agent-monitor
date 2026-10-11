//! `am repo list|add|remove` (src/commands/repo/*.ts).

use std::path::Path;

use am_core::info;
use serde_json::Value;

use crate::output::{self, col, col_right};
use crate::resolve::abs_path;
use crate::{fail, store};

pub fn list(json: bool) {
    let conn = store::open();
    let repos = store::repositories(&conn);
    let worktrees = store::worktrees(&conn);
    let count = |id: &str| worktrees.iter().filter(|w| w.repo_id == id).count();

    if json {
        let rows: Vec<Value> = repos
            .iter()
            .map(|r| {
                let mut v = r.json();
                v["worktreeCount"] = count(&r.id).into();
                v
            })
            .collect();
        output::json(&Value::Array(rows));
        return;
    }
    if repos.is_empty() {
        println!("No repositories tracked. Run: am repo add <path>");
        return;
    }
    let rows: Vec<_> = repos
        .iter()
        .map(|r| {
            vec![
                ("name", r.name.clone()),
                ("path", r.path.clone()),
                ("worktrees", count(&r.id).to_string()),
                ("lastUsed", r.last_used_at.clone()),
            ]
        })
        .collect();
    output::table(
        &rows,
        &[col("name", "Name"), col("path", "Path"), col_right("worktrees", "Worktrees"), col("lastUsed", "Last Used")],
    );
}

pub fn add(path: &str, json: bool) {
    let abs = abs_path(path);
    if !Path::new(&abs).join(".git").exists() {
        fail(&format!("Not a git repository: {abs}"));
    }
    let conn = store::open();
    if let Some(existing) = store::repository_by_path(&conn, &abs) {
        println!("Repository already tracked: {} ({})", existing.name, existing.path);
        return;
    }
    let name = Path::new(&abs).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| abs.clone());
    let repo = store::add_repository(&conn, &abs, &name);
    info!("cli", "Added repository {} ({})", repo.name, repo.path);
    super::lifecycle::sync_new_repo(&conn, &repo.repository());

    if json {
        output::json(&repo.json());
    } else {
        println!("Added repository: {} ({})", repo.name, repo.path);
    }
}

pub fn remove(name_or_path: &str) {
    let conn = store::open();
    let repo = store::repository_by_path(&conn, &abs_path(name_or_path))
        .or_else(|| store::repositories(&conn).into_iter().find(|r| r.name == name_or_path))
        .unwrap_or_else(|| fail(&format!("Repository not found: {name_or_path}")));
    store::remove_repository(&conn, &repo.id);
    info!("cli", "Removed repository {} ({})", repo.name, repo.path);
    println!("Removed repository: {} ({})", repo.name, repo.path);
}
