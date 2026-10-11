//! The handful of CLI-only queries (full repo rows, lookups by path, repo
//! add/remove, the full `agent_status` row). Dashboard reads live in
//! `am_core::db`; these mirror the TS helpers in `src/lib/db.ts`.

use am_core::model::{Repository, Worktree};
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::fail;

pub fn open() -> Connection {
    am_core::db::open().unwrap_or_else(|e| fail(&format!("Failed to open database: {e}")))
}

#[derive(Debug, Clone)]
pub struct RepoRow {
    pub id: String,
    pub path: String,
    pub name: String,
    pub last_used_at: String,
}

impl RepoRow {
    pub fn repository(&self) -> Repository {
        Repository { id: self.id.clone(), path: self.path.clone(), name: self.name.clone() }
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::json!({ "id": self.id, "path": self.path, "name": self.name, "last_used_at": self.last_used_at })
    }
}

fn repo_row(r: &Row) -> rusqlite::Result<RepoRow> {
    Ok(RepoRow { id: r.get(0)?, path: r.get(1)?, name: r.get(2)?, last_used_at: r.get(3)? })
}

const REPO_COLS: &str = "id, path, name, last_used_at";

pub fn repositories(conn: &Connection) -> Vec<RepoRow> {
    let run = || -> rusqlite::Result<Vec<RepoRow>> {
        let mut stmt = conn.prepare(&format!("SELECT {REPO_COLS} FROM repositories ORDER BY last_used_at DESC"))?;
        let rows = stmt.query_map([], repo_row)?;
        rows.collect()
    };
    run().unwrap_or_else(|e| fail(&format!("Database error: {e}")))
}

pub fn repository_by_path(conn: &Connection, path: &str) -> Option<RepoRow> {
    conn.query_row(&format!("SELECT {REPO_COLS} FROM repositories WHERE path = ?1"), [path], repo_row)
        .optional()
        .unwrap_or_else(|e| fail(&format!("Database error: {e}")))
}

/// Port of `addRepository`: insert, or refresh name/last_used_at on a re-add.
pub fn add_repository(conn: &Connection, path: &str, name: &str) -> RepoRow {
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO repositories (id, path, name) VALUES (?1, ?2, ?3)
         ON CONFLICT(path) DO UPDATE SET name = excluded.name, last_used_at = datetime('now')",
        params![id, path, name],
    )
    .unwrap_or_else(|e| fail(&format!("Database error: {e}")));
    repository_by_path(conn, path).unwrap_or_else(|| fail("Failed to add repository"))
}

pub fn remove_repository(conn: &Connection, id: &str) {
    conn.execute("DELETE FROM repositories WHERE id = ?1", [id]).unwrap_or_else(|e| fail(&format!("Database error: {e}")));
}

pub fn worktrees(conn: &Connection) -> Vec<Worktree> {
    am_core::db::worktrees(conn).unwrap_or_else(|e| fail(&format!("Database error: {e}")))
}

pub fn worktree_by_path(conn: &Connection, path: &str) -> Option<Worktree> {
    worktrees(conn).into_iter().find(|w| w.path == path)
}

/// The full `agent_status` row (the dashboard model drops some columns).
#[derive(Debug, Clone, Default)]
pub struct StatusRow {
    pub status: String,
    pub session_id: Option<String>,
    pub last_response: Option<String>,
    pub transcript_summary: Option<String>,
    pub is_open: bool,
    pub updated_at: String,
}

pub fn agent_status(conn: &Connection, worktree_id: &str) -> Option<StatusRow> {
    conn.query_row(
        "SELECT status, session_id, last_response, transcript_summary, is_open, updated_at
         FROM agent_status WHERE worktree_id = ?1",
        [worktree_id],
        |r| {
            Ok(StatusRow {
                status: r.get(0)?,
                session_id: r.get(1)?,
                last_response: r.get(2)?,
                transcript_summary: r.get(3)?,
                is_open: r.get::<_, Option<i64>>(4)?.unwrap_or(0) != 0,
                updated_at: r.get(5)?,
            })
        },
    )
    .optional()
    .unwrap_or_else(|e| fail(&format!("Database error: {e}")))
}

/// Port of `upsertAgentStatus(id, status, null, null, null, isOpen)`: keeps the
/// existing session/response/summary.
pub fn set_agent_status(conn: &Connection, worktree_id: &str, status: &str, is_open: bool) {
    conn.execute(
        "INSERT INTO agent_status (worktree_id, status, is_open, updated_at)
         VALUES (?1, ?2, ?3, datetime('now'))
         ON CONFLICT(worktree_id) DO UPDATE SET
           status = excluded.status,
           is_open = excluded.is_open,
           updated_at = datetime('now')",
        params![worktree_id, status, is_open as i64],
    )
    .unwrap_or_else(|e| fail(&format!("Database error: {e}")));
}
