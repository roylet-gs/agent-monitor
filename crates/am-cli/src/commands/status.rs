//! `am status --worktree <path> [--set <status>]` (src/commands/status.ts).

use std::io::Write;
use std::os::unix::net::UnixStream;
use std::time::Duration;

use am_core::status::Status;
use am_core::{info, paths};
use serde_json::json;

use crate::{fail_stdout, store};

const VALID: [&str; 6] = ["idle", "executing", "planning", "waiting", "delegating", "done"];

pub fn run(worktree_path: &str, set: Option<&str>) {
    let conn = store::open();
    let Some(wt) = store::worktree_by_path(&conn, worktree_path) else {
        fail_stdout(&format!("No worktree found in DB for: {worktree_path}"));
    };

    if let Some(new) = set {
        if Status::parse(new).is_none() || !VALID.contains(&new) {
            fail_stdout(&format!("Invalid status: {new}. Must be one of: {}", VALID.join(", ")));
        }
        store::set_agent_status(&conn, &wt.id, new, true);
        info!("status", "Status set to {new} for {} (CLI)", wt.path);
        publish(&json!({
            "type": "agent-status-update",
            "worktreeId": wt.id,
            "status": new,
            "sessionId": null,
            "lastResponse": null,
            "transcriptSummary": null,
            "isOpen": true,
            "updatedAt": now_iso(),
        }));
        println!("Status set to \"{new}\" for {}", wt.path);
        return;
    }

    let st = store::agent_status(&conn, &wt.id);
    println!("Worktree: {}", wt.path);
    println!("Branch:   {}", wt.branch);
    println!("Status:   {}", st.as_ref().map(|s| s.status.as_str()).unwrap_or("unknown"));
    println!("Session:  {}", st.as_ref().and_then(|s| s.session_id.as_deref()).unwrap_or("none"));
    println!("Open:     {}", if st.as_ref().is_some_and(|s| s.is_open) { "yes" } else { "no" });
    println!("Updated:  {}", st.as_ref().map(|s| s.updated_at.as_str()).unwrap_or("never"));
    if let Some(resp) = st.as_ref().and_then(|s| s.last_response.as_deref()).filter(|r| !r.is_empty()) {
        let truncated = if resp.chars().count() > 200 {
            format!("{}...", resp.chars().take(200).collect::<String>())
        } else {
            resp.to_string()
        };
        println!("Last Response: {truncated}");
    }
}

fn now_iso() -> String {
    time::OffsetDateTime::now_utc()
        .format(time::macros::format_description!(
            "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z"
        ))
        .unwrap_or_default()
}

/// Fire-and-forget nudge to a running TUI; SQLite is the source of truth.
fn publish(msg: &serde_json::Value) {
    let Ok(mut s) = UnixStream::connect(paths::socket_path()) else { return };
    let _ = s.set_write_timeout(Some(Duration::from_millis(200)));
    let _ = s.write_all(format!("{msg}\n").as_bytes());
}
