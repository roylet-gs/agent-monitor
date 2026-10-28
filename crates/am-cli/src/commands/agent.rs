//! `am agent attach <target>`: resume the worktree's latest agent session
//! (Claude Code, Codex, Gemini CLI, Cursor…) in this terminal. The TS version attached to am's own managed (`claude -p`)
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

    let known = store::agent_status(&conn, &wt.id).filter(|a| a.session_id.as_deref().is_some_and(|s| !s.is_empty()));
    let default_agent = am_core::settings::load().default_agent();
    let resume = launch::resume_for(Path::new(&wt.path), known.as_ref().map(|a| a.agent), default_agent);
    if resume.session_id.is_none() && !resume.continue_session {
        // The fresh-start command, e.g. `acli rovodev run` (`resume.start`).
        let p = default_agent.provider();
        let program = [p.program().to_owned()].into_iter().chain(p.resume_args(None, false)).collect::<Vec<_>>().join(" ");
        fail(&format!("No agent session found for {}. Start one with: cd {} && {program}", wt.branch, wt.path));
    }
    let provider = resume.agent.provider();
    // Agents resolve --resume within the project of the cwd, so start where the session did.
    let cwd = resume.cwd.clone().filter(|c| Path::new(c).is_dir()).unwrap_or_else(|| wt.path.clone());
    let args = provider.resume_args(resume.session_id.as_deref(), resume.continue_session);
    info!("cli", "Attaching: {} {} for {} in {cwd}", provider.program(), args.join(" "), wt.branch);
    let err = Command::new(provider.program()).args(&args).current_dir(&cwd).exec();
    fail(&format!("Failed to run {}: {err}", provider.program()));
}
