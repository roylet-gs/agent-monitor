//! `am agent attach <target>`: resume the worktree's latest Claude session in
//! this terminal. The TS version attached to am's own managed (`claude -p`)
//! session; those are cut, so this resumes whatever session was last active
//! at the worktree (root or any subdirectory) — the same one the TUI's `c` uses.

use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;

use am_core::{info, launch};

use crate::resolve::{resolve_repo, resolve_worktree};
use crate::{fail, store};

pub fn attach(target: &str, repo_path: Option<&str>) {
    let conn = store::open();
    let repo_id = repo_path.map(|p| resolve_repo(&conn, Some(p)).id);
    let wt = resolve_worktree(&conn, target, repo_id.as_deref());

    let (session_id, cwd) = match launch::discover_sessions(Path::new(&wt.path)).into_iter().next() {
        Some(s) => (s.id, s.cwd),
        None => match store::agent_status(&conn, &wt.id).and_then(|a| a.session_id).filter(|s| !s.is_empty()) {
            Some(id) => (id, wt.path.clone()),
            None => fail(&format!("No Claude session found for {}. Start one with: cd {} && claude", wt.branch, wt.path)),
        },
    };
    // claude resolves --resume within the project of the cwd, so start where the session did.
    let cwd = if Path::new(&cwd).is_dir() { cwd } else { wt.path.clone() };
    info!("cli", "Attaching to session {session_id} for {} in {cwd}", wt.branch);
    let err = Command::new("claude").args(["--resume", &session_id]).current_dir(&cwd).exec();
    fail(&format!("Failed to run claude: {err}"));
}
