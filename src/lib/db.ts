import Database from "better-sqlite3";
import { existsSync, mkdirSync } from "fs";
import { dirname } from "path";
import { APP_DIR, DB_PATH } from "./paths.js";
import { log } from "./logger.js";
import type { Repository, Worktree, AgentStatus, AgentStatusType } from "./types.js";
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
  log("info", "db", `Database opened at ${DB_PATH}`);
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
      plan_mode INTEGER NOT NULL DEFAULT 0,
      last_response TEXT,
      session_id TEXT,
      updated_at TEXT NOT NULL DEFAULT (datetime('now'))
    );
  `);

  // Migration: add plan_mode column if missing
  try {
    db.prepare("SELECT plan_mode FROM agent_status LIMIT 0").run();
  } catch {
    db.exec("ALTER TABLE agent_status ADD COLUMN plan_mode INTEGER NOT NULL DEFAULT 0");
    log("info", "db", "Migrated agent_status: added plan_mode column");
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
  name: string
): Worktree {
  const db = getDb();
  const id = randomUUID();
  db.prepare(
    `INSERT INTO worktrees (id, repo_id, path, branch, name) VALUES (?, ?, ?, ?, ?)
     ON CONFLICT(repo_id, branch) DO UPDATE SET path = excluded.path, name = excluded.name`
  ).run(id, repoId, path, branch, name);
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

export function updateWorktreeCustomName(id: string, customName: string | null): void {
  getDb()
    .prepare("UPDATE worktrees SET custom_name = ? WHERE id = ?")
    .run(customName, id);
}

// --- Agent Status ---

export function upsertAgentStatus(
  worktreeId: string,
  status: AgentStatusType,
  sessionId?: string | null,
  lastResponse?: string | null,
  planMode?: boolean | null
): void {
  const db = getDb();
  if (planMode != null) {
    // Update plan_mode explicitly
    db.prepare(
      `INSERT INTO agent_status (worktree_id, status, plan_mode, session_id, last_response, updated_at)
       VALUES (?, ?, ?, ?, ?, datetime('now'))
       ON CONFLICT(worktree_id) DO UPDATE SET
         status = excluded.status,
         plan_mode = excluded.plan_mode,
         session_id = COALESCE(excluded.session_id, agent_status.session_id),
         last_response = COALESCE(excluded.last_response, agent_status.last_response),
         updated_at = datetime('now')`
    ).run(worktreeId, status, planMode ? 1 : 0, sessionId ?? null, lastResponse ?? null);
  } else {
    // Don't touch plan_mode
    db.prepare(
      `INSERT INTO agent_status (worktree_id, status, plan_mode, session_id, last_response, updated_at)
       VALUES (?, ?, 0, ?, ?, datetime('now'))
       ON CONFLICT(worktree_id) DO UPDATE SET
         status = excluded.status,
         session_id = COALESCE(excluded.session_id, agent_status.session_id),
         last_response = COALESCE(excluded.last_response, agent_status.last_response),
         updated_at = datetime('now')`
    ).run(worktreeId, status, sessionId ?? null, lastResponse ?? null);
  }
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

export function closeDb(): void {
  if (db) {
    db.close();
    db = null;
  }
}
