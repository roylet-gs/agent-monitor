//! `am script edit|remove|show [--review]` (src/commands/script.ts + src/lib/scripts.ts).
//! `--review` acts on the review script (`<repo-id>.review.sh`) instead of the
//! startup script; both live in `am_core::worktrees`.

use std::path::Path;
use std::process::{Command, Stdio};

use am_core::worktrees::ScriptKind;
use am_core::{info, launch, settings, warn};

use crate::resolve::resolve_repo;
use crate::{fail, output, store};

pub fn kind(review: bool) -> ScriptKind {
    if review { ScriptKind::Review } else { ScriptKind::Startup }
}

/// `Startup script` / `Review script`.
fn title(kind: ScriptKind) -> String {
    let label = kind.label();
    format!("{}{}", label[..1].to_uppercase(), &label[1..])
}

/// Port of `openFileInEditor`: the configured editor (`launch::open_in_editor`),
/// or the system opener for `terminal`.
fn open_file_in_editor(path: &Path, s: &settings::Settings) -> Result<(), String> {
    let ide = launch::IdeChoice::from_setting(&s.ide);
    if !ide.is_terminal() {
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

pub fn edit(repo_path: Option<&str>, kind: ScriptKind) {
    let conn = store::open();
    let repo = resolve_repo(&conn, repo_path);
    let s = settings::load();
    let path = kind.create(&repo.id).unwrap_or_else(|e| fail(&format!("Failed to create {}: {e}", kind.path(&repo.id).display())));
    if let Err(e) = open_file_in_editor(&path, &s) {
        fail(&e);
    }
    println!("Opened {} for {} in {}", kind.label(), repo.name, s.ide);
}

pub fn remove(repo_path: Option<&str>, kind: ScriptKind) {
    let conn = store::open();
    let repo = resolve_repo(&conn, repo_path);
    match kind.remove(&repo.id) {
        Ok(true) => println!("Removed {} for {}", kind.label(), repo.name),
        Ok(false) => println!("No {} for {}", kind.label(), repo.name),
        Err(e) => fail(&format!("Failed to remove {}: {e}", kind.path(&repo.id).display())),
    }
}

pub fn show(repo_path: Option<&str>, kind: ScriptKind, json: bool) {
    let conn = store::open();
    let repo = resolve_repo(&conn, repo_path);
    let path = kind.path(&repo.id);
    if !path.exists() {
        println!("No {} for {}", kind.label(), repo.name);
        return;
    }
    let content = std::fs::read_to_string(&path).unwrap_or_else(|e| fail(&format!("Failed to read {}: {e}", path.display())));
    if json {
        output::json(&serde_json::json!({ "repo": repo.name, "path": path.to_string_lossy(), "content": content }));
        return;
    }
    println!("# {} for {}", title(kind), repo.name);
    println!("# Path: {}", path.display());
    println!();
    println!("{content}");
}
