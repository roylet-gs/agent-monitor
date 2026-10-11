//! SQLite access shared with the TS app (same file, same tables, WAL).
//!
//! Migrations are versioned with `PRAGMA user_version` instead of the TS
//! app's probe-each-column-on-every-open approach, so a hook invocation pays
//! for one pragma read, not a dozen failed SELECTs. Version 1 is a baseline
//! that brings any DB the TS app created up to its latest shape idempotently.

use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};

use crate::paths;
use crate::status::Status;

pub const SCHEMA_VERSION: i64 = 2;

/// A subagent counts as running only if it reported something recently;
/// protects the count from a missed SubagentStop (crash, killed session).
const STALE_SUBAGENT_SQL: &str = "datetime('now', '-2 hours')";

pub fn open() -> rusqlite::Result<Connection> {
    let dir = paths::app_dir();
    let _ = std::fs::create_dir_all(&dir);
    open_at(&paths::db_path())
}

pub fn open_at(path: &std::path::Path) -> rusqlite::Result<Connection> {
    let mut conn = Connection::open(path)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    // Cap the WAL file after checkpoints; it had grown to 18x the DB size.
    conn.pragma_update(None, "journal_size_limit", 4 * 1024 * 1024)?;
    migrate(&mut conn)?;
    Ok(conn)
}

fn user_version(conn: &Connection) -> rusqlite::Result<i64> {
    conn.pragma_query_value(None, "user_version", |r| r.get(0))
}

fn migrate(conn: &mut Connection) -> rusqlite::Result<()> {
    if user_version(conn)? >= SCHEMA_VERSION {
        return Ok(());
    }
    // IMMEDIATE takes the write lock up front so concurrent hook processes
    // serialize here; re-check the version once we hold it.
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let v = user_version(&tx)?;
    if v < 1 {
        baseline(&tx)?;
    }
    if v < 2 {
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS subagents (
               session_id TEXT NOT NULL,
               agent_id TEXT NOT NULL,
               worktree_id TEXT REFERENCES worktrees(id) ON DELETE CASCADE,
               standalone_path TEXT,
               agent_type TEXT,
               description TEXT,
               status TEXT NOT NULL DEFAULT 'running',
               tool_count INTEGER NOT NULL DEFAULT 0,
               last_tool TEXT,
               last_tool_input TEXT,
               last_message TEXT,
               transcript_path TEXT,
               started_at TEXT NOT NULL DEFAULT (datetime('now')),
               ended_at TEXT,
               updated_at TEXT NOT NULL DEFAULT (datetime('now')),
               PRIMARY KEY (session_id, agent_id)
             );
             CREATE INDEX IF NOT EXISTS subagents_worktree ON subagents(worktree_id, status);
             CREATE INDEX IF NOT EXISTS subagents_standalone ON subagents(standalone_path, status);",
        )?;
    }
    tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    tx.commit()?;
    crate::info!("db", "Migrated schema from user_version {v} to {SCHEMA_VERSION}");
    Ok(())
}

fn has_column(tx: &Transaction, table: &str, column: &str) -> rusqlite::Result<bool> {
    let mut stmt = tx.prepare(&format!("PRAGMA table_info({table})"))?;
    let names = stmt.query_map([], |r| r.get::<_, String>(1))?;
    for n in names {
        if n? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The TS app's schema as of v1.0.95, created or completed idempotently.
fn baseline(tx: &Transaction) -> rusqlite::Result<()> {
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS repositories (
           id TEXT PRIMARY KEY,
           path TEXT NOT NULL UNIQUE,
           name TEXT NOT NULL,
           last_used_at TEXT NOT NULL DEFAULT (datetime('now'))
         );
         CREATE TABLE IF NOT EXISTS worktrees (
           id TEXT PRIMARY KEY,
           repo_id TEXT NOT NULL REFERENCES repositories(id) ON DELETE CASCADE,
           path TEXT NOT NULL,
           branch TEXT NOT NULL,
           name TEXT NOT NULL,
           custom_name TEXT,
           created_at TEXT NOT NULL DEFAULT (datetime('now')),
           UNIQUE(repo_id, branch)
         );
         CREATE TABLE IF NOT EXISTS agent_status (
           worktree_id TEXT PRIMARY KEY REFERENCES worktrees(id) ON DELETE CASCADE,
           status TEXT NOT NULL DEFAULT 'idle',
           last_response TEXT,
           transcript_summary TEXT,
           session_id TEXT,
           updated_at TEXT NOT NULL DEFAULT (datetime('now'))
         );
         CREATE TABLE IF NOT EXISTS standalone_sessions (
           id TEXT PRIMARY KEY,
           path TEXT NOT NULL UNIQUE,
           status TEXT NOT NULL DEFAULT 'idle',
           session_id TEXT,
           last_response TEXT,
           transcript_summary TEXT,
           is_open INTEGER NOT NULL DEFAULT 1,
           created_at TEXT NOT NULL DEFAULT (datetime('now')),
           updated_at TEXT NOT NULL DEFAULT (datetime('now'))
         );
         CREATE TABLE IF NOT EXISTS managed_sessions (
           id TEXT PRIMARY KEY,
           worktree_id TEXT NOT NULL UNIQUE REFERENCES worktrees(id) ON DELETE CASCADE,
           cwd TEXT NOT NULL,
           last_prompt TEXT,
           turn_pid INTEGER,
           turn_count INTEGER NOT NULL DEFAULT 0,
           created_at TEXT NOT NULL DEFAULT (datetime('now')),
           updated_at TEXT NOT NULL DEFAULT (datetime('now'))
         );
         CREATE TABLE IF NOT EXISTS integration_cache (
           kind TEXT NOT NULL,
           cache_key TEXT NOT NULL,
           payload TEXT,
           fetched_at TEXT NOT NULL DEFAULT (datetime('now')),
           PRIMARY KEY (kind, cache_key)
         );",
    )?;
    let columns: [(&str, &str, &str); 6] = [
        ("agent_status", "transcript_summary", "TEXT"),
        ("agent_status", "is_open", "INTEGER NOT NULL DEFAULT 0"),
        ("agent_status", "active_subagents", "INTEGER NOT NULL DEFAULT 0"),
        ("standalone_sessions", "active_subagents", "INTEGER NOT NULL DEFAULT 0"),
        ("worktrees", "nickname_source", "TEXT"),
        ("worktrees", "is_main", "INTEGER NOT NULL DEFAULT 0"),
    ];
    for (table, column, decl) in columns {
        if !has_column(tx, table, column)? {
            tx.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {decl}"))?;
        }
    }
    Ok(())
}

/// Which status row an event belongs to: a tracked worktree, or a
/// "standalone" Claude session running somewhere am doesn't manage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Owner {
    Worktree { id: String, repo_id: String, branch: String },
    Standalone { path: String },
}

#[derive(Debug, Clone, Default)]
pub struct StatusRow {
    pub status: Option<Status>,
    pub session_id: Option<String>,
    pub is_open: bool,
    pub active_subagents: i64,
}

pub fn find_worktree(conn: &Connection, path: &str) -> rusqlite::Result<Option<Owner>> {
    conn.query_row(
        "SELECT id, repo_id, branch FROM worktrees WHERE path = ?",
        [path],
        |r| Ok(Owner::Worktree { id: r.get(0)?, repo_id: r.get(1)?, branch: r.get(2)? }),
    )
    .optional()
}

pub fn get_status(conn: &Connection, owner: &Owner) -> rusqlite::Result<Option<StatusRow>> {
    let (sql, key) = match owner {
        Owner::Worktree { id, .. } => (
            "SELECT status, session_id, is_open, active_subagents FROM agent_status WHERE worktree_id = ?",
            id.as_str(),
        ),
        Owner::Standalone { path } => (
            "SELECT status, session_id, is_open, active_subagents FROM standalone_sessions WHERE path = ?",
            path.as_str(),
        ),
    };
    conn.query_row(sql, [key], |r| {
        Ok(StatusRow {
            status: r.get::<_, Option<String>>(0)?.as_deref().and_then(Status::parse),
            session_id: r.get(1)?,
            is_open: r.get::<_, Option<i64>>(2)?.unwrap_or(0) != 0,
            active_subagents: r.get::<_, Option<i64>>(3)?.unwrap_or(0),
        })
    })
    .optional()
}

pub struct StatusUpdate<'a> {
    pub status: Status,
    pub session_id: Option<&'a str>,
    pub last_response: Option<&'a str>,
    pub transcript_summary: Option<&'a str>,
    pub is_open: bool,
}

/// Same COALESCE semantics as the TS `upsertAgentStatus`/`upsertStandaloneSession`:
/// null fields keep the existing value.
pub fn upsert_status(conn: &Connection, owner: &Owner, u: &StatusUpdate) -> rusqlite::Result<()> {
    let open = u.is_open as i64;
    match owner {
        Owner::Worktree { id, .. } => conn.execute(
            "INSERT INTO agent_status (worktree_id, status, session_id, last_response, transcript_summary, is_open, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, datetime('now'))
             ON CONFLICT(worktree_id) DO UPDATE SET
               status = excluded.status,
               session_id = COALESCE(excluded.session_id, agent_status.session_id),
               last_response = COALESCE(excluded.last_response, agent_status.last_response),
               transcript_summary = COALESCE(excluded.transcript_summary, agent_status.transcript_summary),
               is_open = excluded.is_open,
               updated_at = datetime('now')",
            params![id, u.status.as_str(), u.session_id, u.last_response, u.transcript_summary, open],
        )?,
        Owner::Standalone { path } => conn.execute(
            "INSERT INTO standalone_sessions (id, path, status, session_id, last_response, transcript_summary, is_open, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, datetime('now'))
             ON CONFLICT(path) DO UPDATE SET
               status = excluded.status,
               session_id = COALESCE(excluded.session_id, standalone_sessions.session_id),
               last_response = COALESCE(excluded.last_response, standalone_sessions.last_response),
               transcript_summary = COALESCE(excluded.transcript_summary, standalone_sessions.transcript_summary),
               is_open = excluded.is_open,
               updated_at = datetime('now')",
            params![uuid::Uuid::new_v4().to_string(), path, u.status.as_str(), u.session_id, u.last_response, u.transcript_summary, open],
        )?,
    };
    Ok(())
}

/// Mirror the live subagent count into the legacy `active_subagents` column so
/// the TS TUI keeps rendering `delegating` correctly during the migration.
pub fn set_active_subagents(conn: &Connection, owner: &Owner, count: u32) -> rusqlite::Result<()> {
    match owner {
        Owner::Worktree { id, .. } => conn.execute(
            "INSERT INTO agent_status (worktree_id, active_subagents, updated_at) VALUES (?1, ?2, datetime('now'))
             ON CONFLICT(worktree_id) DO UPDATE SET active_subagents = excluded.active_subagents",
            params![id, count],
        )?,
        Owner::Standalone { path } => conn.execute(
            "INSERT INTO standalone_sessions (id, path, active_subagents, updated_at) VALUES (?1, ?2, ?3, datetime('now'))
             ON CONFLICT(path) DO UPDATE SET active_subagents = excluded.active_subagents",
            params![uuid::Uuid::new_v4().to_string(), path, count],
        )?,
    };
    Ok(())
}

fn owner_cols(owner: &Owner) -> (Option<&str>, Option<&str>) {
    match owner {
        Owner::Worktree { id, .. } => (Some(id.as_str()), None),
        Owner::Standalone { path } => (None, Some(path.as_str())),
    }
}

fn owner_where(owner: &Owner) -> (&'static str, &str) {
    match owner {
        Owner::Worktree { id, .. } => ("worktree_id = ?1", id.as_str()),
        Owner::Standalone { path } => ("standalone_path = ?1", path.as_str()),
    }
}

pub struct SubagentKey<'a> {
    pub session_id: &'a str,
    pub agent_id: &'a str,
}

/// SubagentStart (also fired when a finished subagent is resumed): mark running.
pub fn subagent_started(
    conn: &Connection,
    owner: &Owner,
    key: &SubagentKey,
    agent_type: Option<&str>,
) -> rusqlite::Result<()> {
    let (wt, sp) = owner_cols(owner);
    conn.execute(
        "INSERT INTO subagents (session_id, agent_id, worktree_id, standalone_path, agent_type, status)
         VALUES (?1, ?2, ?3, ?4, ?5, 'running')
         ON CONFLICT(session_id, agent_id) DO UPDATE SET
           status = 'running', ended_at = NULL,
           agent_type = COALESCE(excluded.agent_type, subagents.agent_type),
           updated_at = datetime('now')",
        params![key.session_id, key.agent_id, wt, sp, agent_type],
    )?;
    Ok(())
}

/// SubagentStop: mark done. Idempotent — Claude Code may report a stop more
/// than once, and a stop for an agent we never saw start is recorded as done.
pub fn subagent_stopped(
    conn: &Connection,
    owner: &Owner,
    key: &SubagentKey,
    agent_type: Option<&str>,
    transcript_path: Option<&str>,
    last_message: Option<&str>,
) -> rusqlite::Result<()> {
    let (wt, sp) = owner_cols(owner);
    conn.execute(
        "INSERT INTO subagents (session_id, agent_id, worktree_id, standalone_path, agent_type, status, transcript_path, last_message, ended_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 'done', ?6, ?7, datetime('now'))
         ON CONFLICT(session_id, agent_id) DO UPDATE SET
           status = 'done',
           ended_at = COALESCE(subagents.ended_at, datetime('now')),
           agent_type = COALESCE(excluded.agent_type, subagents.agent_type),
           transcript_path = COALESCE(excluded.transcript_path, subagents.transcript_path),
           last_message = COALESCE(excluded.last_message, subagents.last_message),
           updated_at = datetime('now')",
        params![key.session_id, key.agent_id, wt, sp, agent_type, transcript_path, last_message],
    )?;
    Ok(())
}

/// A tool event from inside a subagent: record what it is doing. If we missed
/// its SubagentStart, this (re)creates it as running.
pub fn subagent_tool(
    conn: &Connection,
    owner: &Owner,
    key: &SubagentKey,
    agent_type: Option<&str>,
    tool: Option<&str>,
    tool_input: Option<&str>,
    count_call: bool,
) -> rusqlite::Result<()> {
    let (wt, sp) = owner_cols(owner);
    conn.execute(
        "INSERT INTO subagents (session_id, agent_id, worktree_id, standalone_path, agent_type, status, tool_count, last_tool, last_tool_input)
         VALUES (?1, ?2, ?3, ?4, ?5, 'running', ?6, ?7, ?8)
         ON CONFLICT(session_id, agent_id) DO UPDATE SET
           status = 'running', ended_at = NULL,
           tool_count = subagents.tool_count + ?6,
           last_tool = COALESCE(excluded.last_tool, subagents.last_tool),
           last_tool_input = COALESCE(excluded.last_tool_input, subagents.last_tool_input),
           agent_type = COALESCE(excluded.agent_type, subagents.agent_type),
           updated_at = datetime('now')",
        params![key.session_id, key.agent_id, wt, sp, agent_type, count_call as i64, tool, tool_input],
    )?;
    Ok(())
}

/// Fill in description/type/transcript from Claude Code's per-agent files.
pub fn set_subagent_meta(
    conn: &Connection,
    key: &SubagentKey,
    description: Option<&str>,
    agent_type: Option<&str>,
    transcript_path: Option<&str>,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE subagents SET
           description = COALESCE(?3, description),
           agent_type = COALESCE(agent_type, ?4),
           transcript_path = COALESCE(transcript_path, ?5)
         WHERE session_id = ?1 AND agent_id = ?2",
        params![key.session_id, key.agent_id, description, agent_type, transcript_path],
    )?;
    Ok(())
}

pub fn subagent_has_description(conn: &Connection, key: &SubagentKey) -> rusqlite::Result<bool> {
    Ok(conn
        .query_row(
            "SELECT description IS NOT NULL FROM subagents WHERE session_id = ?1 AND agent_id = ?2",
            params![key.session_id, key.agent_id],
            |r| r.get::<_, bool>(0),
        )
        .optional()?
        .unwrap_or(false))
}

/// Close out subagents that can no longer be running: on SessionEnd for that
/// session, and on SessionStart for every *other* session at this owner.
pub fn end_subagents(conn: &Connection, owner: &Owner, session_id: &str, other_sessions: bool) -> rusqlite::Result<usize> {
    let (owner_sql, key) = owner_where(owner);
    let cmp = if other_sessions { "!=" } else { "=" };
    conn.execute(
        &format!(
            "UPDATE subagents SET status = 'done', ended_at = COALESCE(ended_at, datetime('now')), updated_at = datetime('now')
             WHERE {owner_sql} AND status = 'running' AND session_id {cmp} ?2"
        ),
        params![key, session_id],
    )
}

pub fn running_subagents(conn: &Connection, owner: &Owner) -> rusqlite::Result<u32> {
    let (owner_sql, key) = owner_where(owner);
    conn.query_row(
        &format!(
            "SELECT COUNT(*) FROM subagents WHERE {owner_sql} AND status = 'running' AND updated_at > {STALE_SUBAGENT_SQL}"
        ),
        [key],
        |r| r.get(0),
    )
}

pub fn touch_status(conn: &Connection, owner: &Owner) -> rusqlite::Result<()> {
    match owner {
        Owner::Worktree { id, .. } => {
            conn.execute("UPDATE agent_status SET updated_at = datetime('now') WHERE worktree_id = ?", [id])?
        }
        Owner::Standalone { path } => {
            conn.execute("UPDATE standalone_sessions SET updated_at = datetime('now') WHERE path = ?", [path])?
        }
    };
    Ok(())
}

/// Opportunistic, non-blocking checkpoint so the WAL can't grow unbounded
/// while a long-lived TS reader holds it open.
pub fn checkpoint_passive(conn: &Connection) {
    let _ = conn.query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |_| Ok(()));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> (tempfile::TempDir, Connection) {
        let dir = tempfile::tempdir().unwrap();
        let conn = open_at(&dir.path().join("t.db")).unwrap();
        conn.execute("INSERT INTO repositories (id, path, name) VALUES ('r', '/repo', 'repo')", []).unwrap();
        conn.execute("INSERT INTO worktrees (id, repo_id, path, branch, name) VALUES ('w', 'r', '/repo/wt', 'feat', 'feat')", []).unwrap();
        (dir, conn)
    }

    fn wt() -> Owner {
        Owner::Worktree { id: "w".into(), repo_id: "r".into(), branch: "feat".into() }
    }

    #[test]
    fn migrates_a_legacy_ts_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("legacy.db");
        {
            // Shape of a pre-is_open TS database.
            let c = Connection::open(&path).unwrap();
            c.execute_batch(
                "CREATE TABLE worktrees (id TEXT PRIMARY KEY, repo_id TEXT, path TEXT, branch TEXT, name TEXT, custom_name TEXT, created_at TEXT);
                 CREATE TABLE agent_status (worktree_id TEXT PRIMARY KEY, status TEXT, last_response TEXT, session_id TEXT, updated_at TEXT);",
            )
            .unwrap();
        }
        let conn = open_at(&path).unwrap();
        assert_eq!(user_version(&conn).unwrap(), SCHEMA_VERSION);
        let tx_cols: Vec<String> = conn
            .prepare("PRAGMA table_info(agent_status)")
            .unwrap()
            .query_map([], |r| r.get(1))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        for c in ["transcript_summary", "is_open", "active_subagents"] {
            assert!(tx_cols.iter().any(|n| n == c), "missing {c}");
        }
        // Re-opening is a no-op.
        drop(conn);
        open_at(&path).unwrap();
    }

    #[test]
    fn subagent_lifecycle_counts_by_id_not_by_event() {
        let (_d, conn) = fresh();
        let o = wt();
        let k = |id| SubagentKey { session_id: "s", agent_id: id };
        for id in ["a", "b", "c", "d"] {
            subagent_started(&conn, &o, &k(id), Some("general-purpose")).unwrap();
        }
        assert_eq!(running_subagents(&conn, &o).unwrap(), 4);
        // Duplicate stops for the same agent (seen in real logs) only count once.
        subagent_stopped(&conn, &o, &k("a"), None, None, None).unwrap();
        subagent_stopped(&conn, &o, &k("a"), None, None, None).unwrap();
        subagent_stopped(&conn, &o, &k("a"), None, None, None).unwrap();
        // A stop for an agent we never saw start does not affect the others.
        subagent_stopped(&conn, &o, &k("zzz"), Some("Explore"), None, None).unwrap();
        assert_eq!(running_subagents(&conn, &o).unwrap(), 3);
        // A resumed agent is running again.
        subagent_started(&conn, &o, &k("a"), None).unwrap();
        assert_eq!(running_subagents(&conn, &o).unwrap(), 4);
    }

    #[test]
    fn subagent_tool_events_track_progress() {
        let (_d, conn) = fresh();
        let o = wt();
        let key = SubagentKey { session_id: "s", agent_id: "a" };
        subagent_tool(&conn, &o, &key, Some("Explore"), Some("Bash"), Some("pnpm test"), true).unwrap();
        subagent_tool(&conn, &o, &key, None, Some("Bash"), None, false).unwrap();
        subagent_tool(&conn, &o, &key, None, Some("Read"), Some("a.rs"), true).unwrap();
        let (n, tool, input): (i64, String, String) = conn
            .query_row("SELECT tool_count, last_tool, last_tool_input FROM subagents", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap();
        assert_eq!((n, tool.as_str(), input.as_str()), (2, "Read", "a.rs"));
    }

    #[test]
    fn session_boundaries_close_stale_subagents() {
        let (_d, conn) = fresh();
        let o = wt();
        subagent_started(&conn, &o, &SubagentKey { session_id: "old", agent_id: "a" }, None).unwrap();
        subagent_started(&conn, &o, &SubagentKey { session_id: "new", agent_id: "b" }, None).unwrap();
        assert_eq!(end_subagents(&conn, &o, "new", true).unwrap(), 1);
        assert_eq!(running_subagents(&conn, &o).unwrap(), 1);
        assert_eq!(end_subagents(&conn, &o, "new", false).unwrap(), 1);
        assert_eq!(running_subagents(&conn, &o).unwrap(), 0);
    }

    #[test]
    fn status_upsert_keeps_existing_fields_when_null() {
        let (_d, conn) = fresh();
        let o = wt();
        upsert_status(&conn, &o, &StatusUpdate { status: Status::Executing, session_id: Some("s1"), last_response: None, transcript_summary: None, is_open: true }).unwrap();
        upsert_status(&conn, &o, &StatusUpdate { status: Status::Done, session_id: None, last_response: Some("ok"), transcript_summary: None, is_open: true }).unwrap();
        let row = get_status(&conn, &o).unwrap().unwrap();
        assert_eq!(row.status, Some(Status::Done));
        assert_eq!(row.session_id.as_deref(), Some("s1"));
        set_active_subagents(&conn, &o, 2).unwrap();
        assert_eq!(get_status(&conn, &o).unwrap().unwrap().active_subagents, 2);
    }

    #[test]
    fn standalone_owner_round_trips() {
        let (_d, conn) = fresh();
        let o = Owner::Standalone { path: "/tmp/x".into() };
        set_active_subagents(&conn, &o, 1).unwrap();
        upsert_status(&conn, &o, &StatusUpdate { status: Status::Waiting, session_id: Some("s"), last_response: None, transcript_summary: None, is_open: true }).unwrap();
        let row = get_status(&conn, &o).unwrap().unwrap();
        assert_eq!(row.status, Some(Status::Waiting));
        assert_eq!(row.active_subagents, 1);
    }
}
