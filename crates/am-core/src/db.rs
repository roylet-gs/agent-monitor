//! SQLite access shared with the TS app (same file, same tables, WAL).
//!
//! Migrations are versioned with `PRAGMA user_version` instead of the TS
//! app's probe-each-column-on-every-open approach, so a hook invocation pays
//! for one pragma read, not a dozen failed SELECTs. Version 1 is a baseline
//! that brings any DB the TS app created up to its latest shape idempotently.

use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};

use crate::harness::HarnessKind;
use crate::paths;
use crate::status::Status;

pub const SCHEMA_VERSION: i64 = 5;

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
    if v < 3 {
        // Which agent CLI (`crate::harness::HarnessKind` id) the session belongs
        // to. NULL = Claude: every row 1.x or an older 2.x wrote.
        for table in ["agent_status", "standalone_sessions"] {
            if !has_column(&tx, table, "agent")? {
                tx.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN agent TEXT"))?;
            }
        }
    }
    if v < 4 {
        // One row per agent session, so several sessions in one worktree each
        // keep their own status. `agent_status` / `standalone_sessions` stay as
        // the per-owner headline (most urgent session) for 1.x and older 2.x.
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS sessions (
               agent TEXT NOT NULL,
               session_id TEXT NOT NULL,
               worktree_id TEXT REFERENCES worktrees(id) ON DELETE CASCADE,
               standalone_path TEXT,
               status TEXT NOT NULL DEFAULT 'idle',
               last_response TEXT,
               is_open INTEGER NOT NULL DEFAULT 1,
               started_at TEXT NOT NULL DEFAULT (datetime('now')),
               updated_at TEXT NOT NULL DEFAULT (datetime('now')),
               ended_at TEXT,
               PRIMARY KEY (agent, session_id)
             );
             CREATE INDEX IF NOT EXISTS sessions_worktree ON sessions(worktree_id, is_open);
             CREATE INDEX IF NOT EXISTS sessions_standalone ON sessions(standalone_path, is_open);",
        )?;
    }
    if v < 5 {
        // Written by the removed plugin training mode; kept (unused) because
        // migrations are additive and 1.x/older 2.x builds share this DB.
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS plugin_issues (
               plugin TEXT NOT NULL,
               kind TEXT NOT NULL,
               key TEXT NOT NULL,
               sample TEXT,
               count INTEGER NOT NULL DEFAULT 1,
               first_seen TEXT NOT NULL DEFAULT (datetime('now')),
               last_seen TEXT NOT NULL DEFAULT (datetime('now')),
               PRIMARY KEY (plugin, kind, key)
             );",
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
/// "standalone" agent session running somewhere am doesn't manage.
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
    pub agent: HarnessKind,
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
            "SELECT status, session_id, is_open, active_subagents, agent FROM agent_status WHERE worktree_id = ?",
            id.as_str(),
        ),
        Owner::Standalone { path } => (
            "SELECT status, session_id, is_open, active_subagents, agent FROM standalone_sessions WHERE path = ?",
            path.as_str(),
        ),
    };
    conn.query_row(sql, [key], |r| {
        Ok(StatusRow {
            status: r.get::<_, Option<String>>(0)?.as_deref().and_then(Status::parse),
            session_id: r.get(1)?,
            is_open: r.get::<_, Option<i64>>(2)?.unwrap_or(0) != 0,
            active_subagents: r.get::<_, Option<i64>>(3)?.unwrap_or(0),
            agent: HarnessKind::from_db(r.get::<_, Option<String>>(4)?.as_deref()),
        })
    })
    .optional()
}

pub struct StatusUpdate<'a> {
    pub status: Status,
    /// The agent that sent the event; `None` keeps the stored one.
    pub agent: Option<HarnessKind>,
    pub session_id: Option<&'a str>,
    pub last_response: Option<&'a str>,
    pub transcript_summary: Option<&'a str>,
    pub is_open: bool,
}

/// Same COALESCE semantics as the TS `upsertAgentStatus`/`upsertStandaloneSession`:
/// null fields keep the existing value.
pub fn upsert_status(conn: &Connection, owner: &Owner, u: &StatusUpdate) -> rusqlite::Result<()> {
    let open = u.is_open as i64;
    let agent = u.agent.map(HarnessKind::id);
    match owner {
        Owner::Worktree { id, .. } => conn.execute(
            "INSERT INTO agent_status (worktree_id, status, session_id, last_response, transcript_summary, is_open, agent, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, datetime('now'))
             ON CONFLICT(worktree_id) DO UPDATE SET
               status = excluded.status,
               agent = COALESCE(excluded.agent, agent_status.agent),
               session_id = COALESCE(excluded.session_id, agent_status.session_id),
               last_response = COALESCE(excluded.last_response, agent_status.last_response),
               transcript_summary = COALESCE(excluded.transcript_summary, agent_status.transcript_summary),
               is_open = excluded.is_open,
               updated_at = datetime('now')",
            params![id, u.status.as_str(), u.session_id, u.last_response, u.transcript_summary, open, agent],
        )?,
        Owner::Standalone { path } => conn.execute(
            "INSERT INTO standalone_sessions (id, path, status, session_id, last_response, transcript_summary, is_open, agent, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, datetime('now'))
             ON CONFLICT(path) DO UPDATE SET
               status = excluded.status,
               agent = COALESCE(excluded.agent, standalone_sessions.agent),
               session_id = COALESCE(excluded.session_id, standalone_sessions.session_id),
               last_response = COALESCE(excluded.last_response, standalone_sessions.last_response),
               transcript_summary = COALESCE(excluded.transcript_summary, standalone_sessions.transcript_summary),
               is_open = excluded.is_open,
               updated_at = datetime('now')",
            params![uuid::Uuid::new_v4().to_string(), path, u.status.as_str(), u.session_id, u.last_response, u.transcript_summary, open, agent],
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
           -- tool and input move together: a new tool without a summarisable
           -- input must not keep showing the previous tool's input
           last_tool_input = CASE WHEN excluded.last_tool IS NOT NULL THEN excluded.last_tool_input
                                  ELSE subagents.last_tool_input END,
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
/// session, and on SessionStart for every *other* session at this owner that
/// isn't still open. Another session running in the same worktree keeps its
/// subagents; one that ended (or was never recorded in `sessions`, like a
/// session from before schema v4) loses them.
pub fn end_subagents(conn: &Connection, owner: &Owner, session_id: &str, other_sessions: bool) -> rusqlite::Result<usize> {
    let (owner_sql, key) = owner_where(owner);
    let which = if other_sessions {
        "session_id != ?2 AND session_id NOT IN (SELECT session_id FROM sessions WHERE is_open = 1)"
    } else {
        "session_id = ?2"
    };
    conn.execute(
        &format!(
            "UPDATE subagents SET status = 'done', ended_at = COALESCE(ended_at, datetime('now')), updated_at = datetime('now')
             WHERE {owner_sql} AND status = 'running' AND {which}"
        ),
        params![key, session_id],
    )
}

/// Live subagents of one session at this owner.
pub fn running_subagents_in(conn: &Connection, owner: &Owner, session_id: &str) -> rusqlite::Result<u32> {
    let (owner_sql, key) = owner_where(owner);
    conn.query_row(
        &format!(
            "SELECT COUNT(*) FROM subagents
             WHERE {owner_sql} AND session_id = ?2 AND status = 'running' AND updated_at > {STALE_SUBAGENT_SQL}"
        ),
        params![key, session_id],
        |r| r.get(0),
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

// --- Sessions (schema v4) ---

/// One agent session's stored state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRow {
    pub agent: HarnessKind,
    pub session_id: String,
    pub status: Status,
    pub last_response: Option<String>,
    pub is_open: bool,
    pub started_at: String,
    pub updated_at: String,
}

fn row_to_session(r: &rusqlite::Row) -> rusqlite::Result<SessionRow> {
    Ok(SessionRow {
        agent: HarnessKind::from_db(r.get::<_, Option<String>>(0)?.as_deref()),
        session_id: r.get(1)?,
        status: r.get::<_, Option<String>>(2)?.as_deref().and_then(Status::parse).unwrap_or(Status::Idle),
        last_response: r.get(3)?,
        is_open: r.get::<_, Option<i64>>(4)?.unwrap_or(0) != 0,
        started_at: r.get(5)?,
        updated_at: r.get(6)?,
    })
}

const SESSION_COLS: &str = "agent, session_id, status, last_response, is_open, started_at, updated_at";

pub fn get_session(conn: &Connection, agent: HarnessKind, session_id: &str) -> rusqlite::Result<Option<SessionRow>> {
    conn.query_row(
        &format!("SELECT {SESSION_COLS} FROM sessions WHERE agent = ?1 AND session_id = ?2"),
        params![agent.id(), session_id],
        row_to_session,
    )
    .optional()
}

/// Insert or update one session. `last_response = None` keeps the stored
/// one; the owner follows the latest event (a session can move worktree).
pub fn upsert_session(
    conn: &Connection,
    owner: &Owner,
    agent: HarnessKind,
    session_id: &str,
    status: Status,
    last_response: Option<&str>,
    is_open: bool,
) -> rusqlite::Result<()> {
    let (wt, sp) = owner_cols(owner);
    conn.execute(
        "INSERT INTO sessions (agent, session_id, worktree_id, standalone_path, status, last_response, is_open, ended_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, CASE WHEN ?7 = 0 THEN datetime('now') END)
         ON CONFLICT(agent, session_id) DO UPDATE SET
           worktree_id = excluded.worktree_id,
           standalone_path = excluded.standalone_path,
           status = excluded.status,
           last_response = COALESCE(excluded.last_response, sessions.last_response),
           is_open = excluded.is_open,
           ended_at = CASE WHEN excluded.is_open = 0 THEN COALESCE(sessions.ended_at, datetime('now')) END,
           updated_at = datetime('now')",
        params![agent.id(), session_id, wt, sp, status.as_str(), last_response, is_open as i64],
    )?;
    Ok(())
}

pub fn touch_session(conn: &Connection, agent: HarnessKind, session_id: &str) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE sessions SET updated_at = datetime('now') WHERE agent = ?1 AND session_id = ?2",
        params![agent.id(), session_id],
    )?;
    Ok(())
}

/// Every session recorded at this owner, most recently active first.
pub fn sessions_at(conn: &Connection, owner: &Owner) -> rusqlite::Result<Vec<SessionRow>> {
    let (owner_sql, key) = owner_where(owner);
    let mut stmt = conn.prepare(&format!(
        "SELECT {SESSION_COLS} FROM sessions WHERE {owner_sql} ORDER BY updated_at DESC, started_at DESC"
    ))?;
    let rows = stmt.query_map([key], row_to_session)?;
    rows.collect()
}

/// Every session of `agent` the hooks recorded, with the directory it ran in
/// (its worktree's path, else the standalone path) and when it last reported,
/// most recent first. Resume discovery for harnesses whose own session store
/// am can't read.
pub fn recorded_sessions(conn: &Connection, agent: HarnessKind) -> rusqlite::Result<Vec<(String, String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT s.session_id, COALESCE(w.path, s.standalone_path), s.updated_at
         FROM sessions s LEFT JOIN worktrees w ON w.id = s.worktree_id
         WHERE s.agent = ?1 AND COALESCE(w.path, s.standalone_path) IS NOT NULL
         ORDER BY s.updated_at DESC, s.started_at DESC",
    )?;
    let rows = stmt.query_map(params![agent.id()], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
    rows.collect()
}

/// Open sessions of every tracked worktree for the dashboard: worktree id →
/// sessions, oldest first (so rows keep their order as sessions update).
pub fn open_worktree_sessions(conn: &Connection) -> rusqlite::Result<std::collections::HashMap<String, Vec<SessionRow>>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT worktree_id, {SESSION_COLS} FROM sessions
         WHERE worktree_id IS NOT NULL AND is_open = 1
         ORDER BY started_at, session_id"
    ))?;
    let mut out: std::collections::HashMap<String, Vec<SessionRow>> = std::collections::HashMap::new();
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let wt: String = r.get(0)?;
        let s = SessionRow {
            agent: HarnessKind::from_db(r.get::<_, Option<String>>(1)?.as_deref()),
            session_id: r.get(2)?,
            status: r.get::<_, Option<String>>(3)?.as_deref().and_then(Status::parse).unwrap_or(Status::Idle),
            last_response: r.get(4)?,
            is_open: r.get::<_, Option<i64>>(5)?.unwrap_or(0) != 0,
            started_at: r.get(6)?,
            updated_at: r.get(7)?,
        };
        out.entry(wt).or_default().push(s);
    }
    Ok(out)
}

/// Closed sessions idle for a day are history; drop them so the table stays small.
pub fn prune_sessions(conn: &Connection) -> rusqlite::Result<usize> {
    conn.execute("DELETE FROM sessions WHERE is_open = 0 AND updated_at < datetime('now', '-1 day')", [])
}

/// Opportunistic, non-blocking checkpoint so the WAL can't grow unbounded
/// while a long-lived TS reader holds it open.
pub fn checkpoint_passive(conn: &Connection) {
    let _ = conn.query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |_| Ok(()));
}

// --- Dashboard reads ---

pub fn repositories(conn: &Connection) -> rusqlite::Result<Vec<crate::model::Repository>> {
    let mut stmt = conn.prepare("SELECT id, path, name FROM repositories ORDER BY last_used_at DESC")?;
    let rows = stmt.query_map([], |r| Ok(crate::model::Repository { id: r.get(0)?, path: r.get(1)?, name: r.get(2)? }))?;
    rows.collect()
}

pub fn worktrees(conn: &Connection) -> rusqlite::Result<Vec<crate::model::Worktree>> {
    let mut stmt = conn.prepare(
        "SELECT id, repo_id, path, branch, name, custom_name, is_main, created_at FROM worktrees ORDER BY created_at DESC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(crate::model::Worktree {
            id: r.get(0)?,
            repo_id: r.get(1)?,
            path: r.get(2)?,
            branch: r.get(3)?,
            name: r.get(4)?,
            custom_name: r.get(5)?,
            is_main: r.get::<_, Option<i64>>(6)?.unwrap_or(0) != 0,
            created_at: r.get(7)?,
        })
    })?;
    rows.collect()
}

pub fn agent_states(conn: &Connection) -> rusqlite::Result<std::collections::HashMap<String, crate::model::AgentState>> {
    let mut stmt = conn.prepare(
        "SELECT worktree_id, status, session_id, last_response, is_open, active_subagents, updated_at, agent FROM agent_status",
    )?;
    let rows = stmt.query_map([], |r| {
        let status = r.get::<_, Option<String>>(1)?.as_deref().and_then(Status::parse).unwrap_or(Status::Idle);
        Ok((
            r.get::<_, String>(0)?,
            crate::model::AgentState {
                status,
                session_id: r.get(2)?,
                last_response: r.get(3)?,
                is_open: r.get::<_, Option<i64>>(4)?.unwrap_or(0) != 0,
                active_subagents: r.get::<_, Option<i64>>(5)?.unwrap_or(0),
                updated_at: r.get(6)?,
                agent: HarnessKind::from_db(r.get::<_, Option<String>>(7)?.as_deref()),
            },
        ))
    })?;
    rows.collect()
}

/// Subagents for the dashboard pane: every running one plus those that
/// finished within `recent_minutes`, newest first. `worktree_id = None`
/// returns them for all worktrees (the global feed).
pub fn subagents(
    conn: &Connection,
    worktree_id: Option<&str>,
    recent_minutes: u32,
) -> rusqlite::Result<Vec<(Option<String>, crate::model::Subagent)>> {
    let sql = format!(
        "SELECT worktree_id, session_id, agent_id, agent_type, description, status, tool_count, last_tool,
                last_tool_input, started_at, ended_at, updated_at, transcript_path
         FROM subagents
         WHERE (?1 IS NULL OR worktree_id = ?1)
           AND worktree_id IS NOT NULL
           AND ((status = 'running' AND updated_at > {STALE_SUBAGENT_SQL})
                OR ended_at > datetime('now', ?2))
         ORDER BY (status = 'running') DESC, started_at DESC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let window = format!("-{recent_minutes} minutes");
    let rows = stmt.query_map(params![worktree_id, window], |r| {
        Ok((
            r.get::<_, Option<String>>(0)?,
            crate::model::Subagent {
                session_id: r.get(1)?,
                agent_id: r.get(2)?,
                agent_type: r.get(3)?,
                description: r.get(4)?,
                running: r.get::<_, String>(5)? == "running",
                tool_count: r.get(6)?,
                last_tool: r.get(7)?,
                last_tool_input: r.get(8)?,
                started_at: r.get(9)?,
                ended_at: r.get(10)?,
                updated_at: r.get(11)?,
                transcript_path: r.get(12)?,
            },
        ))
    })?;
    rows.collect()
}

// --- Repository / worktree writes (worktree lifecycle) ---
//
// Same SQL as the TS `src/lib/db.ts` functions named in each doc comment:
// both apps write these rows, so conflict handling must stay identical.

const WORKTREE_COLS: &str = "id, repo_id, path, branch, name, custom_name, is_main, created_at";

fn row_to_worktree(r: &rusqlite::Row) -> rusqlite::Result<crate::model::Worktree> {
    Ok(crate::model::Worktree {
        id: r.get(0)?,
        repo_id: r.get(1)?,
        path: r.get(2)?,
        branch: r.get(3)?,
        name: r.get(4)?,
        custom_name: r.get(5)?,
        is_main: r.get::<_, Option<i64>>(6)?.unwrap_or(0) != 0,
        created_at: r.get(7)?,
    })
}

fn row_to_repository(r: &rusqlite::Row) -> rusqlite::Result<crate::model::Repository> {
    Ok(crate::model::Repository { id: r.get(0)?, path: r.get(1)?, name: r.get(2)? })
}

/// `addRepository`: insert, or on an existing path refresh its name and
/// `last_used_at` (the id is kept). Returns the stored row.
pub fn add_repository(conn: &Connection, path: &str, name: &str) -> rusqlite::Result<crate::model::Repository> {
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO repositories (id, path, name) VALUES (?1, ?2, ?3)
         ON CONFLICT(path) DO UPDATE SET name = excluded.name, last_used_at = datetime('now')",
        params![id, path, name],
    )?;
    repository_by_path(conn, path)?.ok_or(rusqlite::Error::QueryReturnedNoRows)
}

/// `getRepositoryByPath`.
pub fn repository_by_path(conn: &Connection, path: &str) -> rusqlite::Result<Option<crate::model::Repository>> {
    conn.query_row("SELECT id, path, name FROM repositories WHERE path = ?1", [path], row_to_repository)
        .optional()
}

/// `getRepositoryById`.
pub fn repository_by_id(conn: &Connection, id: &str) -> rusqlite::Result<Option<crate::model::Repository>> {
    conn.query_row("SELECT id, path, name FROM repositories WHERE id = ?1", [id], row_to_repository)
        .optional()
}

/// `touchRepository`.
pub fn touch_repository(conn: &Connection, id: &str) -> rusqlite::Result<()> {
    conn.execute("UPDATE repositories SET last_used_at = datetime('now') WHERE id = ?1", [id])?;
    Ok(())
}

/// `removeRepository`. Worktrees (and their status/session rows) go with it
/// via `ON DELETE CASCADE` (`foreign_keys` is on for every connection).
pub fn remove_repository(conn: &Connection, id: &str) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM repositories WHERE id = ?1", [id])?;
    Ok(())
}

/// `upsertWorktree`: insert, or on an existing `(repo_id, branch)` update
/// path/name/is_main (id, custom_name and created_at are kept). A path that
/// was tracked as a standalone session is promoted: its standalone row goes.
pub fn upsert_worktree(
    conn: &Connection,
    repo_id: &str,
    path: &str,
    branch: &str,
    name: &str,
    is_main: bool,
) -> rusqlite::Result<crate::model::Worktree> {
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO worktrees (id, repo_id, path, branch, name, is_main) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(repo_id, branch) DO UPDATE SET path = excluded.path, name = excluded.name, is_main = excluded.is_main",
        params![id, repo_id, path, branch, name, is_main as i64],
    )?;
    conn.execute("DELETE FROM standalone_sessions WHERE path = ?1", [path])?;
    worktree_by_branch(conn, repo_id, branch)?.ok_or(rusqlite::Error::QueryReturnedNoRows)
}

/// `getWorktrees`: one repo's worktrees, newest first.
pub fn worktrees_for_repo(conn: &Connection, repo_id: &str) -> rusqlite::Result<Vec<crate::model::Worktree>> {
    let mut stmt =
        conn.prepare(&format!("SELECT {WORKTREE_COLS} FROM worktrees WHERE repo_id = ?1 ORDER BY created_at DESC"))?;
    let rows = stmt.query_map([repo_id], row_to_worktree)?;
    rows.collect()
}

/// `getWorktreeByBranch`.
pub fn worktree_by_branch(
    conn: &Connection,
    repo_id: &str,
    branch: &str,
) -> rusqlite::Result<Option<crate::model::Worktree>> {
    conn.query_row(
        &format!("SELECT {WORKTREE_COLS} FROM worktrees WHERE repo_id = ?1 AND branch = ?2"),
        [repo_id, branch],
        row_to_worktree,
    )
    .optional()
}

/// `getWorktreeByPath`.
pub fn worktree_by_path(conn: &Connection, path: &str) -> rusqlite::Result<Option<crate::model::Worktree>> {
    conn.query_row(&format!("SELECT {WORKTREE_COLS} FROM worktrees WHERE path = ?1"), [path], row_to_worktree)
        .optional()
}

/// `removeWorktree`.
pub fn remove_worktree(conn: &Connection, id: &str) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM worktrees WHERE id = ?1", [id])?;
    Ok(())
}

/// `updateWorktreeCustomName`: set (or clear with `None`) the nickname and
/// where it came from (`None` = user, `Some("linear")` = auto-nickname).
pub fn update_worktree_custom_name(
    conn: &Connection,
    id: &str,
    custom_name: Option<&str>,
    source: Option<&str>,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE worktrees SET custom_name = ?1, nickname_source = ?2 WHERE id = ?3",
        params![custom_name, source, id],
    )?;
    Ok(())
}

/// `clearLinearNicknames`: drop every nickname the Linear auto-nickname
/// feature set (user-set nicknames are kept). Returns how many were cleared.
pub fn clear_linear_nicknames(conn: &Connection) -> rusqlite::Result<usize> {
    conn.execute(
        "UPDATE worktrees SET custom_name = NULL, nickname_source = NULL WHERE nickname_source = 'linear'",
        [],
    )
}

/// Agent sessions running outside any tracked worktree:
/// (row id, path, state), most recently active first.
pub fn standalone_sessions(conn: &Connection) -> rusqlite::Result<Vec<(String, String, crate::model::AgentState)>> {
    let mut stmt = conn.prepare(
        "SELECT id, path, status, session_id, last_response, is_open, active_subagents, updated_at, agent
         FROM standalone_sessions ORDER BY updated_at DESC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            crate::model::AgentState {
                status: r.get::<_, Option<String>>(2)?.as_deref().and_then(Status::parse).unwrap_or(Status::Idle),
                session_id: r.get(3)?,
                last_response: r.get(4)?,
                is_open: r.get::<_, Option<i64>>(5)?.unwrap_or(0) != 0,
                active_subagents: r.get::<_, Option<i64>>(6)?.unwrap_or(0),
                updated_at: r.get(7)?,
                agent: HarnessKind::from_db(r.get::<_, Option<String>>(8)?.as_deref()),
            },
        ))
    })?;
    rows.collect()
}

pub fn remove_standalone_session(conn: &Connection, path: &str) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM standalone_sessions WHERE path = ?", [path])?;
    Ok(())
}

/// `pruneStaleStandaloneSessions`: closed sessions idle for over an hour.
pub fn prune_standalone_sessions(conn: &Connection) -> rusqlite::Result<usize> {
    conn.execute("DELETE FROM standalone_sessions WHERE is_open = 0 AND updated_at < datetime('now', '-1 hour')", [])
}

/// Cheap change detector for the TUI: bumps whenever any connection commits.
pub fn data_version(conn: &Connection) -> rusqlite::Result<i64> {
    conn.pragma_query_value(None, "data_version", |r| r.get(0))
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
        for c in ["transcript_summary", "is_open", "active_subagents", "agent"] {
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
        subagent_tool(&conn, &o, &key, None, Some("SubagentHandback"), None, true).unwrap();
        let cleared: Option<String> = conn.query_row("SELECT last_tool_input FROM subagents", [], |r| r.get(0)).unwrap();
        assert_eq!(cleared, None);
        subagent_tool(&conn, &o, &key, None, Some("Read"), Some("a.rs"), false).unwrap();
        let (n, tool, input): (i64, String, String) = conn
            .query_row("SELECT tool_count, last_tool, last_tool_input FROM subagents", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap();
        assert_eq!((n, tool.as_str(), input.as_str()), (3, "Read", "a.rs"));
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
        upsert_status(&conn, &o, &StatusUpdate { status: Status::Executing, session_id: Some("s1"), last_response: None, transcript_summary: None, is_open: true, agent: None }).unwrap();
        upsert_status(&conn, &o, &StatusUpdate { status: Status::Done, session_id: None, last_response: Some("ok"), transcript_summary: None, is_open: true, agent: None }).unwrap();
        let row = get_status(&conn, &o).unwrap().unwrap();
        assert_eq!(row.status, Some(Status::Done));
        assert_eq!(row.session_id.as_deref(), Some("s1"));
        set_active_subagents(&conn, &o, 2).unwrap();
        assert_eq!(get_status(&conn, &o).unwrap().unwrap().active_subagents, 2);
    }

    #[test]
    fn status_rows_remember_the_agent() {
        let (_d, conn) = fresh();
        let up = |owner: &Owner, agent| {
            let u = StatusUpdate { status: Status::Executing, session_id: None, last_response: None, transcript_summary: None, is_open: true, agent };
            upsert_status(&conn, owner, &u).unwrap();
        };
        let id = |o: &Owner| match o {
            Owner::Worktree { id, .. } => id.clone(),
            Owner::Standalone { .. } => unreachable!(),
        };
        up(&wt(), None);
        assert_eq!(agent_states(&conn).unwrap()[&id(&wt())].agent, HarnessKind::CLAUDE);
        up(&wt(), Some(HarnessKind::GEMINI));
        up(&wt(), None); // e.g. `am status --set`: keeps the agent
        assert_eq!(agent_states(&conn).unwrap()[&id(&wt())].agent, HarnessKind::GEMINI);
        let standalone = Owner::Standalone { path: "/elsewhere".into() };
        up(&standalone, Some(HarnessKind::GEMINI));
        assert_eq!(standalone_sessions(&conn).unwrap()[0].2.agent, HarnessKind::GEMINI);
    }

    #[test]
    fn dashboard_reads() {
        let (_d, conn) = fresh();
        assert_eq!(repositories(&conn).unwrap().len(), 1);
        let wts = worktrees(&conn).unwrap();
        assert_eq!(wts[0].branch, "feat");
        upsert_status(&conn, &wt(), &StatusUpdate { status: Status::Delegating, session_id: Some("s"), last_response: None, transcript_summary: None, is_open: true, agent: None }).unwrap();
        assert_eq!(agent_states(&conn).unwrap()["w"].status, Status::Delegating);
        subagent_started(&conn, &wt(), &SubagentKey { session_id: "s", agent_id: "a" }, Some("Explore")).unwrap();
        subagent_stopped(&conn, &wt(), &SubagentKey { session_id: "s", agent_id: "b" }, None, None, None).unwrap();
        let subs = subagents(&conn, Some("w"), 30).unwrap();
        assert_eq!(subs.len(), 2);
        assert!(subs[0].1.running);
        assert_eq!(subagents(&conn, None, 30).unwrap().len(), 2);
        assert_eq!(subagents(&conn, Some("other"), 30).unwrap().len(), 0);
    }

    #[test]
    fn standalone_owner_round_trips() {
        let (_d, conn) = fresh();
        let o = Owner::Standalone { path: "/tmp/x".into() };
        set_active_subagents(&conn, &o, 1).unwrap();
        upsert_status(&conn, &o, &StatusUpdate { status: Status::Waiting, session_id: Some("s"), last_response: None, transcript_summary: None, is_open: true, agent: None }).unwrap();
        let row = get_status(&conn, &o).unwrap().unwrap();
        assert_eq!(row.status, Some(Status::Waiting));
        assert_eq!(row.active_subagents, 1);
    }

    #[test]
    fn repository_and_worktree_writes_match_ts_semantics() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open_at(&dir.path().join("t.db")).unwrap();
        let r = add_repository(&conn, "/repo", "repo").unwrap();
        // Re-adding the same path keeps the id and refreshes the name.
        let again = add_repository(&conn, "/repo", "renamed").unwrap();
        assert_eq!((again.id.as_str(), again.name.as_str()), (r.id.as_str(), "renamed"));
        assert_eq!(repository_by_id(&conn, &r.id).unwrap().unwrap().path, "/repo");
        touch_repository(&conn, &r.id).unwrap();

        conn.execute("INSERT INTO standalone_sessions (id, path) VALUES ('s', '/repo/wt')", []).unwrap();
        let w = upsert_worktree(&conn, &r.id, "/repo/wt", "feature/x", "x", false).unwrap();
        let standalone: i64 = conn.query_row("SELECT COUNT(*) FROM standalone_sessions", [], |r| r.get(0)).unwrap();
        assert_eq!(standalone, 0, "upsert promotes a standalone session at the same path");

        update_worktree_custom_name(&conn, &w.id, Some("Nick"), Some("linear")).unwrap();
        // Upsert on the same (repo, branch) keeps id + custom_name, updates path/is_main.
        let w2 = upsert_worktree(&conn, &r.id, "/repo/moved", "feature/x", "x", true).unwrap();
        assert_eq!(w2.id, w.id);
        assert_eq!(w2.custom_name.as_deref(), Some("Nick"));
        assert_eq!((w2.path.as_str(), w2.is_main), ("/repo/moved", true));
        assert_eq!(worktree_by_path(&conn, "/repo/moved").unwrap().unwrap().id, w.id);

        assert_eq!(clear_linear_nicknames(&conn).unwrap(), 1);
        assert_eq!(worktree_by_branch(&conn, &r.id, "feature/x").unwrap().unwrap().custom_name, None);

        upsert_worktree(&conn, &r.id, "/repo", "main", "main", true).unwrap();
        assert_eq!(worktrees_for_repo(&conn, &r.id).unwrap().len(), 2);
        remove_worktree(&conn, &w.id).unwrap();
        assert_eq!(worktrees_for_repo(&conn, &r.id).unwrap().len(), 1);

        // Removing the repo cascades to its worktrees.
        remove_repository(&conn, &r.id).unwrap();
        assert!(repository_by_path(&conn, "/repo").unwrap().is_none());
        let left: i64 = conn.query_row("SELECT COUNT(*) FROM worktrees", [], |r| r.get(0)).unwrap();
        assert_eq!(left, 0);
    }
}
