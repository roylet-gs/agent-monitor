import Database from "better-sqlite3";
import { existsSync, mkdirSync, unlinkSync } from "fs";
import { dirname } from "path";
import { APP_DIR, DB_PATH } from "./paths.js";
import { log } from "./logger.js";
import type { Repository, Worktree, AgentStatus, AgentStatusType, AgentSession } from "./types.js";
import { randomUUID } from "crypto";

let db: Database.Database | null = null;

export function getDb(): Database.Database {
  if (db) return db;
  if (!existsSync(APP_DIR)) {
    mkdirSync(APP_DIR, { recursive: true });
  }
  db = new Database(DB_PATH);
  db.pragma("journal_mode = WAL");
  db.pragma("foreign_keys = ON");
  initSchema(db);
  return db;
}

function initSchema(db: Database.Database): void {
  db.exec(`
    CREATE TABLE IF NOT EXISTS repositories (
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
  `);

  // Migration: drop plan_mode column if present (recreate table without it)
  try {
    db.prepare("SELECT plan_mode FROM agent_status LIMIT 0").run();
    // Column exists — migrate it away
    db.exec(`
      CREATE TABLE agent_status_new (
        worktree_id TEXT PRIMARY KEY REFERENCES worktrees(id) ON DELETE CASCADE,
        status TEXT NOT NULL DEFAULT 'idle',
        last_response TEXT,
        session_id TEXT,
        updated_at TEXT NOT NULL DEFAULT (datetime('now'))
      );
      INSERT INTO agent_status_new (worktree_id, status, last_response, session_id, updated_at)
        SELECT worktree_id, status, last_response, session_id, updated_at FROM agent_status;
      DROP TABLE agent_status;
      ALTER TABLE agent_status_new RENAME TO agent_status;
    `);
    log("info", "db", "Migrated agent_status: dropped plan_mode column");
  } catch {
    // Column doesn't exist — nothing to migrate
  }

  // Migration: add transcript_summary column if missing
  try {
    db.prepare("SELECT transcript_summary FROM agent_status LIMIT 0").run();
  } catch {
    db.exec("ALTER TABLE agent_status ADD COLUMN transcript_summary TEXT");
    log("info", "db", "Migrated agent_status: added transcript_summary column");
  }

  // Migration: add is_open column if missing
  try {
    db.prepare("SELECT is_open FROM agent_status LIMIT 0").run();
  } catch {
    db.exec("ALTER TABLE agent_status ADD COLUMN is_open INTEGER NOT NULL DEFAULT 0");
    log("info", "db", "Migrated agent_status: added is_open column");
  }

  // Migration: add nickname_source column if missing
  try {
    db.prepare("SELECT nickname_source FROM worktrees LIMIT 0").run();
  } catch {
    db.exec("ALTER TABLE worktrees ADD COLUMN nickname_source TEXT");
    log("info", "db", "Migrated worktrees: added nickname_source column");
  }

  // Migration: add is_main column if missing
  try {
    db.prepare("SELECT is_main FROM worktrees LIMIT 0").run();
  } catch {
    db.exec("ALTER TABLE worktrees ADD COLUMN is_main INTEGER NOT NULL DEFAULT 0");
    log("info", "db", "Migrated worktrees: added is_main column");
  }

  // Migration: create agent_sessions table
  try {
    db.prepare("SELECT id FROM agent_sessions LIMIT 0").run();
  } catch {
    db.exec(`
      CREATE TABLE IF NOT EXISTS agent_sessions (
        id TEXT PRIMARY KEY,
        worktree_id TEXT NOT NULL REFERENCES worktrees(id) ON DELETE CASCADE,
        session_id TEXT,
        role_name TEXT,
        status TEXT NOT NULL DEFAULT 'idle',
        last_response TEXT,
        transcript_summary TEXT,
        pid INTEGER,
        is_open INTEGER NOT NULL DEFAULT 0,
        launched_at TEXT NOT NULL DEFAULT (datetime('now')),
        updated_at TEXT NOT NULL DEFAULT (datetime('now'))
      );
      CREATE INDEX IF NOT EXISTS idx_agent_sessions_worktree ON agent_sessions(worktree_id);
      CREATE INDEX IF NOT EXISTS idx_agent_sessions_session ON agent_sessions(worktree_id, session_id);
    `);
    log("info", "db", "Created agent_sessions table");
  }
}

// --- Repositories ---

export function addRepository(path: string, name: string): Repository {
  const db = getDb();
  const id = randomUUID();
  db.prepare(
    `INSERT INTO repositories (id, path, name) VALUES (?, ?, ?)
     ON CONFLICT(path) DO UPDATE SET name = excluded.name, last_used_at = datetime('now')`
  ).run(id, path, name);
  return getRepositoryByPath(path)!;
}

export function getRepositories(): Repository[] {
  return getDb()
    .prepare("SELECT * FROM repositories ORDER BY last_used_at DESC")
    .all() as Repository[];
}

export function getRepositoryByPath(path: string): Repository | undefined {
  return getDb()
    .prepare("SELECT * FROM repositories WHERE path = ?")
    .get(path) as Repository | undefined;
}

export function getRepositoryById(id: string): Repository | undefined {
  return getDb()
    .prepare("SELECT * FROM repositories WHERE id = ?")
    .get(id) as Repository | undefined;
}

export function touchRepository(id: string): void {
  getDb()
    .prepare("UPDATE repositories SET last_used_at = datetime('now') WHERE id = ?")
    .run(id);
}

export function removeRepository(id: string): void {
  getDb().prepare("DELETE FROM repositories WHERE id = ?").run(id);
}

// --- Worktrees ---

export function upsertWorktree(
  repoId: string,
  path: string,
  branch: string,
  name: string,
  isMain = false
): Worktree {
  const db = getDb();
  const id = randomUUID();
  db.prepare(
    `INSERT INTO worktrees (id, repo_id, path, branch, name, is_main) VALUES (?, ?, ?, ?, ?, ?)
     ON CONFLICT(repo_id, branch) DO UPDATE SET path = excluded.path, name = excluded.name, is_main = excluded.is_main`
  ).run(id, repoId, path, branch, name, isMain ? 1 : 0);
  return getWorktreeByBranch(repoId, branch)!;
}

export function getWorktrees(repoId: string): Worktree[] {
  return getDb()
    .prepare("SELECT * FROM worktrees WHERE repo_id = ? ORDER BY created_at DESC")
    .all(repoId) as Worktree[];
}

export function getWorktreeByPath(path: string): Worktree | undefined {
  return getDb()
    .prepare("SELECT * FROM worktrees WHERE path = ?")
    .get(path) as Worktree | undefined;
}

export function getWorktreeByBranch(repoId: string, branch: string): Worktree | undefined {
  return getDb()
    .prepare("SELECT * FROM worktrees WHERE repo_id = ? AND branch = ?")
    .get(repoId, branch) as Worktree | undefined;
}

export function removeWorktree(id: string): void {
  getDb().prepare("DELETE FROM worktrees WHERE id = ?").run(id);
}

export function removeWorktreesForRepo(repoId: string): void {
  getDb().prepare("DELETE FROM worktrees WHERE repo_id = ?").run(repoId);
}

export function updateWorktreeCustomName(id: string, customName: string | null, source: string | null = null): void {
  getDb()
    .prepare("UPDATE worktrees SET custom_name = ?, nickname_source = ? WHERE id = ?")
    .run(customName, source, id);
}

export function clearLinearNicknames(): void {
  const result = getDb()
    .prepare("UPDATE worktrees SET custom_name = NULL, nickname_source = NULL WHERE nickname_source = 'linear'")
    .run();
  if (result.changes > 0) {
    log("info", "db", `Cleared ${result.changes} Linear auto-set nickname(s)`);
  }
}

// --- Agent Status ---

export function upsertAgentStatus(
  worktreeId: string,
  status: AgentStatusType,
  sessionId?: string | null,
  lastResponse?: string | null,
  transcriptSummary?: string | null,
  isOpen?: boolean | null
): void {
  getDb()
    .prepare(
      `INSERT INTO agent_status (worktree_id, status, session_id, last_response, transcript_summary, is_open, updated_at)
       VALUES (?, ?, ?, ?, ?, COALESCE(?, 0), datetime('now'))
       ON CONFLICT(worktree_id) DO UPDATE SET
         status = excluded.status,
         session_id = COALESCE(excluded.session_id, agent_status.session_id),
         last_response = COALESCE(excluded.last_response, agent_status.last_response),
         transcript_summary = COALESCE(excluded.transcript_summary, agent_status.transcript_summary),
         is_open = COALESCE(?, agent_status.is_open),
         updated_at = datetime('now')`
    )
    .run(worktreeId, status, sessionId ?? null, lastResponse ?? null, transcriptSummary ?? null, isOpen != null ? (isOpen ? 1 : 0) : null, isOpen != null ? (isOpen ? 1 : 0) : null);
}

export function getAgentStatus(worktreeId: string): AgentStatus | undefined {
  return getDb()
    .prepare("SELECT * FROM agent_status WHERE worktree_id = ?")
    .get(worktreeId) as AgentStatus | undefined;
}

export function getAgentStatuses(repoId: string): Map<string, AgentStatus> {
  const rows = getDb()
    .prepare(
      `SELECT a.* FROM agent_status a
       JOIN worktrees w ON w.id = a.worktree_id
       WHERE w.repo_id = ?`
    )
    .all(repoId) as AgentStatus[];
  const map = new Map<string, AgentStatus>();
  for (const row of rows) {
    map.set(row.worktree_id, row);
  }
  return map;
}

export function getAllWorktrees(): Worktree[] {
  return getDb()
    .prepare("SELECT * FROM worktrees ORDER BY repo_id, created_at DESC")
    .all() as Worktree[];
}

export function getAllAgentStatuses(): Map<string, AgentStatus> {
  const rows = getDb()
    .prepare("SELECT * FROM agent_status")
    .all() as AgentStatus[];
  const map = new Map<string, AgentStatus>();
  for (const row of rows) {
    map.set(row.worktree_id, row);
  }
  return map;
}

// --- Agent Sessions ---

export function createAgentSession(worktreeId: string, roleName?: string | null): AgentSession {
  const db = getDb();
  const id = randomUUID();
  db.prepare(
    `INSERT INTO agent_sessions (id, worktree_id, role_name, status, is_open, launched_at, updated_at)
     VALUES (?, ?, ?, 'idle', 1, datetime('now'), datetime('now'))`
  ).run(id, worktreeId, roleName ?? null);
  return db.prepare("SELECT * FROM agent_sessions WHERE id = ?").get(id) as AgentSession;
}

export function getAgentSessions(worktreeId: string): AgentSession[] {
  return getDb()
    .prepare("SELECT * FROM agent_sessions WHERE worktree_id = ? ORDER BY launched_at DESC")
    .all(worktreeId) as AgentSession[];
}

export function upsertAgentSession(
  worktreeId: string,
  sessionId: string | null,
  status: AgentStatusType,
  lastResponse?: string | null,
  transcriptSummary?: string | null,
  isOpen?: boolean | null
): string | null {
  const db = getDb();

  // 1. Try matching by (worktree_id, session_id) if session_id is present
  if (sessionId) {
    const exact = db.prepare(
      "SELECT id FROM agent_sessions WHERE worktree_id = ? AND session_id = ?"
    ).get(worktreeId, sessionId) as { id: string } | undefined;

    if (exact) {
      db.prepare(
        `UPDATE agent_sessions SET
          status = ?,
          last_response = COALESCE(?, last_response),
          transcript_summary = COALESCE(?, transcript_summary),
          is_open = COALESCE(?, is_open),
          updated_at = datetime('now')
        WHERE id = ?`
      ).run(status, lastResponse ?? null, transcriptSummary ?? null, isOpen != null ? (isOpen ? 1 : 0) : null, exact.id);
      return exact.id;
    }

    // 2. Try claiming a NULL-session_id placeholder for this worktree
    const placeholder = db.prepare(
      "SELECT id FROM agent_sessions WHERE worktree_id = ? AND session_id IS NULL ORDER BY launched_at DESC LIMIT 1"
    ).get(worktreeId) as { id: string } | undefined;

    if (placeholder) {
      db.prepare(
        `UPDATE agent_sessions SET
          session_id = ?,
          status = ?,
          last_response = COALESCE(?, last_response),
          transcript_summary = COALESCE(?, transcript_summary),
          is_open = COALESCE(?, is_open),
          updated_at = datetime('now')
        WHERE id = ?`
      ).run(sessionId, status, lastResponse ?? null, transcriptSummary ?? null, isOpen != null ? (isOpen ? 1 : 0) : null, placeholder.id);
      return placeholder.id;
    }

    // 3. Insert new row (session started outside am)
    const id = randomUUID();
    db.prepare(
      `INSERT INTO agent_sessions (id, worktree_id, session_id, status, last_response, transcript_summary, is_open, launched_at, updated_at)
       VALUES (?, ?, ?, ?, ?, ?, COALESCE(?, 0), datetime('now'), datetime('now'))`
    ).run(id, worktreeId, sessionId, status, lastResponse ?? null, transcriptSummary ?? null, isOpen != null ? (isOpen ? 1 : 0) : null);
    return id;
  }

  // session_id is NULL: update the most recent session for this worktree
  const recent = db.prepare(
    "SELECT id FROM agent_sessions WHERE worktree_id = ? ORDER BY launched_at DESC LIMIT 1"
  ).get(worktreeId) as { id: string } | undefined;

  if (recent) {
    db.prepare(
      `UPDATE agent_sessions SET
        status = ?,
        last_response = COALESCE(?, last_response),
        transcript_summary = COALESCE(?, transcript_summary),
        is_open = COALESCE(?, is_open),
        updated_at = datetime('now')
      WHERE id = ?`
    ).run(status, lastResponse ?? null, transcriptSummary ?? null, isOpen != null ? (isOpen ? 1 : 0) : null, recent.id);
    return recent.id;
  }

  return null;
}

export function updateAgentSessionPid(id: string, pid: number | null): void {
  getDb()
    .prepare("UPDATE agent_sessions SET pid = ? WHERE id = ?")
    .run(pid, id);
}

export function removeAgentSession(id: string): void {
  getDb().prepare("DELETE FROM agent_sessions WHERE id = ?").run(id);
}

export function clearStalePids(): void {
  const result = getDb()
    .prepare("UPDATE agent_sessions SET pid = NULL WHERE pid IS NOT NULL")
    .run();
  if (result.changes > 0) {
    log("info", "db", `Cleared ${result.changes} stale PID(s) from agent_sessions`);
  }
}

export function closeDb(): void {
  if (db) {
    db.close();
    db = null;
  }
}

export function resetAll(): void {
  closeDb();
  if (existsSync(DB_PATH)) {
    unlinkSync(DB_PATH);
  }
  if (existsSync(DB_PATH + "-wal")) {
    unlinkSync(DB_PATH + "-wal");
  }
  if (existsSync(DB_PATH + "-shm")) {
    unlinkSync(DB_PATH + "-shm");
  }
}
