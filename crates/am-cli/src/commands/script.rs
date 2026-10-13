//! `am script edit|remove|show` (src/commands/script.ts + src/lib/scripts.ts).

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use am_core::{info, launch, paths, settings, warn};

use crate::resolve::resolve_repo;
use crate::{fail, output, store};

const TEMPLATE: &str = "#!/bin/bash
# Startup script for worktree creation
# This runs automatically after a new worktree is created.
# The working directory is set to the new worktree path.

# Example: install dependencies
# npm install

# Example: set up environment
# cp .env.example .env

echo \"Startup script finished.\"
";

pub fn script_path(repo_id: &str) -> PathBuf {
    paths::app_dir().join("scripts").join(format!("{repo_id}.sh"))
}

fn create_startup_script(repo_id: &str) -> PathBuf {
    let path = script_path(repo_id);
    if !path.exists() {
        let write = || -> std::io::Result<()> {
            std::fs::create_dir_all(path.parent().unwrap_or(Path::new("/")))?;
            std::fs::write(&path, TEMPLATE)?;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
        };
        if let Err(e) = write() {
            fail(&format!("Failed to create {}: {e}", path.display()));
        }
        info!("scripts", "Created startup script: {}", path.display());
    }
    path
}

/// Port of `openFileInEditor`: the configured editor (`launch::open_in_editor`),
/// or the system opener for `terminal`.
fn open_file_in_editor(path: &Path, s: &settings::Settings) -> Result<(), String> {
    let ide = launch::IdeChoice::from_setting(&s.ide);
    if ide != launch::IdeChoice::Terminal {
        return launch::open_in_editor(path, ide, &s.ide_command);
    }
    let mut cmd = if cfg!(target_os = "macos") {
        let mut c = Command::new("open");
        c.args(["-a", "Terminal"]);
        c
    } else {
        Command::new("xdg-open")
    };
    let ok = cmd.arg(path).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
    if ok {
        info!("scripts", "Opened file in terminal: {}", path.display());
        Ok(())
    } else {
        warn!("scripts", "Failed to open file in terminal: {}", path.display());
        Err("Failed to open file in terminal.".into())
    }
}

pub fn edit(repo_path: Option<&str>) {
    let conn = store::open();
    let repo = resolve_repo(&conn, repo_path);
    let s = settings::load();
    let path = create_startup_script(&repo.id);
    if let Err(e) = open_file_in_editor(&path, &s) {
        fail(&e);
    }
    println!("Opened startup script for {} in {}", repo.name, s.ide);
}

pub fn remove(repo_path: Option<&str>) {
    let conn = store::open();
    let repo = resolve_repo(&conn, repo_path);
    let path = script_path(&repo.id);
    if !path.exists() {
        println!("No startup script for {}", repo.name);
        return;
    }
    if let Err(e) = std::fs::remove_file(&path) {
        fail(&format!("Failed to remove {}: {e}", path.display()));
    }
    info!("scripts", "Removed startup script: {}", path.display());
    println!("Removed startup script for {}", repo.name);
}

pub fn show(repo_path: Option<&str>, json: bool) {
    let conn = store::open();
    let repo = resolve_repo(&conn, repo_path);
    let path = script_path(&repo.id);
    if !path.exists() {
        println!("No startup script for {}", repo.name);
        return;
    }
    let content = std::fs::read_to_string(&path).unwrap_or_else(|e| fail(&format!("Failed to read {}: {e}", path.display())));
    if json {
        output::json(&serde_json::json!({ "repo": repo.name, "path": path.to_string_lossy(), "content": content }));
        return;
    }
    println!("# Startup script for {}", repo.name);
    println!("# Path: {}", path.display());
    println!();
    println!("{content}");
}
