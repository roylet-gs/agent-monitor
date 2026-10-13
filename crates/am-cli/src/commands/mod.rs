pub mod agent;
pub mod doctor;
pub mod harness;
pub mod hooks;
pub mod lifecycle;
pub mod linear;
pub mod logs;
pub mod pr;
pub mod repo;
pub mod script;
pub mod settings;
pub mod status;
pub mod theme;
pub mod tui;
pub mod worktree;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// `git rev-parse --abbrev-ref HEAD` in `dir` (the TS `getGit(cwd).raw(...)`).
pub fn current_branch(dir: &Path) -> Option<String> {
    let out = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(dir)
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Branch to look up for `pr`/`linear`: the target worktree's, or the CWD's.
pub fn target_branch(conn: &rusqlite::Connection, target: Option<&str>, repo_id: &str) -> String {
    match target {
        Some(t) => crate::resolve::resolve_worktree(conn, t, Some(repo_id)).branch,
        None => {
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            current_branch(&cwd).unwrap_or_else(|| crate::fail("Could not determine the current branch"))
        }
    }
}

/// Open a URL in the default browser (the npm `open` package).
pub fn open_url(url: &str) -> Result<(), String> {
    let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    let status = Command::new(opener)
        .arg(url)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| format!("failed to run {opener}: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("{opener} exited with {status}")) }
}

/// First executable called `name` next to this binary, then on `$PATH`.
pub fn find_sibling_or_path(name: &str) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let exec = |p: &Path| std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0);
    let sibling = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join(name)));
    if let Some(s) = sibling.filter(|s| exec(s)) {
        return Some(s);
    }
    std::env::split_paths(&std::env::var_os("PATH")?).map(|d| d.join(name)).find(|p| exec(p))
}
