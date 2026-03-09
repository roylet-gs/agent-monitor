import { describe, it, expect, vi, beforeEach } from "vitest";

vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

describe("db", () => {
  let db: typeof import("../../src/lib/db.js");

  beforeEach(async () => {
    db = await import("../../src/lib/db.js");
  });

  describe("repositories", () => {
    it("adds a repository", () => {
      const repo = db.addRepository("/tmp/test-repo", "test-repo");
      expect(repo.path).toBe("/tmp/test-repo");
      expect(repo.name).toBe("test-repo");
      expect(repo.id).toBeDefined();
    });

    it("upserts on duplicate path", () => {
      db.addRepository("/tmp/test-repo", "original");
      const updated = db.addRepository("/tmp/test-repo", "updated");
      expect(updated.name).toBe("updated");
      expect(db.getRepositories()).toHaveLength(1);
    });

    it("lists repositories ordered by last_used_at desc", () => {
      const a = db.addRepository("/tmp/repo-a", "a");
      db.addRepository("/tmp/repo-b", "b");
      // Touch "a" to make it most recent
      db.touchRepository(a.id);
      const repos = db.getRepositories();
      expect(repos).toHaveLength(2);
      expect(repos[0]!.name).toBe("a");
    });

    it("finds repository by path", () => {
      db.addRepository("/tmp/test-repo", "test-repo");
      const found = db.getRepositoryByPath("/tmp/test-repo");
      expect(found).toBeDefined();
      expect(found!.name).toBe("test-repo");
    });

    it("finds repository by id", () => {
      const repo = db.addRepository("/tmp/test-repo", "test-repo");
      const found = db.getRepositoryById(repo.id);
      expect(found).toBeDefined();
      expect(found!.path).toBe("/tmp/test-repo");
    });

    it("returns undefined for non-existent path", () => {
      expect(db.getRepositoryByPath("/nope")).toBeUndefined();
    });

    it("removes repository", () => {
      const repo = db.addRepository("/tmp/test-repo", "test-repo");
      db.removeRepository(repo.id);
      expect(db.getRepositoryByPath("/tmp/test-repo")).toBeUndefined();
    });

    it("touches repository updates last_used_at", () => {
      const repo = db.addRepository("/tmp/test-repo", "test-repo");
      const before = db.getRepositoryById(repo.id)!.last_used_at;
      db.touchRepository(repo.id);
      const after = db.getRepositoryById(repo.id)!.last_used_at;
      expect(after).toBeDefined();
      // At least not null
      expect(after.length).toBeGreaterThan(0);
    });
  });

  describe("worktrees", () => {
    it("upserts a worktree", () => {
      const repo = db.addRepository("/tmp/repo", "repo");
      const wt = db.upsertWorktree(repo.id, "/tmp/repo/.worktrees/feat", "feature/test", "test");
      expect(wt.branch).toBe("feature/test");
      expect(wt.name).toBe("test");
    });

    it("upserts on duplicate repo_id+branch", () => {
      const repo = db.addRepository("/tmp/repo", "repo");
      db.upsertWorktree(repo.id, "/tmp/old-path", "feature/test", "test");
      db.upsertWorktree(repo.id, "/tmp/new-path", "feature/test", "test-updated");
      const worktrees = db.getWorktrees(repo.id);
      expect(worktrees).toHaveLength(1);
      expect(worktrees[0]!.path).toBe("/tmp/new-path");
      expect(worktrees[0]!.name).toBe("test-updated");
    });

    it("lists worktrees for a repo", () => {
      const repo = db.addRepository("/tmp/repo", "repo");
      db.upsertWorktree(repo.id, "/tmp/repo/.worktrees/a", "feature/a", "a");
      db.upsertWorktree(repo.id, "/tmp/repo/.worktrees/b", "feature/b", "b");
      expect(db.getWorktrees(repo.id)).toHaveLength(2);
    });

    it("finds worktree by path", () => {
      const repo = db.addRepository("/tmp/repo", "repo");
      db.upsertWorktree(repo.id, "/tmp/repo/.worktrees/feat", "feature/test", "test");
      const found = db.getWorktreeByPath("/tmp/repo/.worktrees/feat");
      expect(found).toBeDefined();
      expect(found!.branch).toBe("feature/test");
    });

    it("finds worktree by branch", () => {
      const repo = db.addRepository("/tmp/repo", "repo");
      db.upsertWorktree(repo.id, "/tmp/repo/.worktrees/feat", "feature/test", "test");
      const found = db.getWorktreeByBranch(repo.id, "feature/test");
      expect(found).toBeDefined();
    });

    it("removes a worktree", () => {
      const repo = db.addRepository("/tmp/repo", "repo");
      const wt = db.upsertWorktree(repo.id, "/tmp/repo/.worktrees/feat", "feature/test", "test");
      db.removeWorktree(wt.id);
      expect(db.getWorktreeByPath("/tmp/repo/.worktrees/feat")).toBeUndefined();
    });

    it("removes all worktrees for a repo", () => {
      const repo = db.addRepository("/tmp/repo", "repo");
      db.upsertWorktree(repo.id, "/tmp/a", "feature/a", "a");
      db.upsertWorktree(repo.id, "/tmp/b", "feature/b", "b");
      db.removeWorktreesForRepo(repo.id);
      expect(db.getWorktrees(repo.id)).toHaveLength(0);
    });

    it("updates custom name", () => {
      const repo = db.addRepository("/tmp/repo", "repo");
      const wt = db.upsertWorktree(repo.id, "/tmp/repo/.worktrees/feat", "feature/test", "test");
      db.updateWorktreeCustomName(wt.id, "My Custom Name");
      const updated = db.getWorktreeByPath("/tmp/repo/.worktrees/feat");
      expect(updated!.custom_name).toBe("My Custom Name");
    });

    it("getAllWorktrees returns all across repos", () => {
      const repo1 = db.addRepository("/tmp/repo1", "repo1");
      const repo2 = db.addRepository("/tmp/repo2", "repo2");
      db.upsertWorktree(repo1.id, "/tmp/a", "feature/a", "a");
      db.upsertWorktree(repo2.id, "/tmp/b", "feature/b", "b");
      expect(db.getAllWorktrees()).toHaveLength(2);
    });
  });

  describe("agent_status", () => {
    it("upserts agent status", () => {
      const repo = db.addRepository("/tmp/repo", "repo");
      const wt = db.upsertWorktree(repo.id, "/tmp/wt", "main", "main");
      db.upsertAgentStatus(wt.id, "executing", "session-1", "hello", "summary", true);
      const status = db.getAgentStatus(wt.id);
      expect(status).toBeDefined();
      expect(status!.status).toBe("executing");
      expect(status!.session_id).toBe("session-1");
      expect(status!.last_response).toBe("hello");
      expect(status!.transcript_summary).toBe("summary");
      expect(status!.is_open).toBe(1);
    });

    it("COALESCE: null fields preserve existing values", () => {
      const repo = db.addRepository("/tmp/repo", "repo");
      const wt = db.upsertWorktree(repo.id, "/tmp/wt", "main", "main");
      db.upsertAgentStatus(wt.id, "executing", "session-1", "hello", "summary", true);
      // Update status without overwriting session/response/summary/is_open
      db.upsertAgentStatus(wt.id, "idle");
      const status = db.getAgentStatus(wt.id);
      expect(status!.status).toBe("idle");
      expect(status!.session_id).toBe("session-1");
      expect(status!.last_response).toBe("hello");
      expect(status!.transcript_summary).toBe("summary");
      expect(status!.is_open).toBe(1);
    });

    it("is_open defaults to 0 on first insert without explicit value", () => {
      const repo = db.addRepository("/tmp/repo", "repo");
      const wt = db.upsertWorktree(repo.id, "/tmp/wt", "main", "main");
      db.upsertAgentStatus(wt.id, "idle");
      const status = db.getAgentStatus(wt.id);
      expect(status!.is_open).toBe(0);
    });

    it("is_open can be set to false explicitly", () => {
      const repo = db.addRepository("/tmp/repo", "repo");
      const wt = db.upsertWorktree(repo.id, "/tmp/wt", "main", "main");
      db.upsertAgentStatus(wt.id, "executing", null, null, null, true);
      expect(db.getAgentStatus(wt.id)!.is_open).toBe(1);
      db.upsertAgentStatus(wt.id, "idle", null, null, null, false);
      expect(db.getAgentStatus(wt.id)!.is_open).toBe(0);
    });

    it("gets agent statuses for a repo", () => {
      const repo = db.addRepository("/tmp/repo", "repo");
      const wt1 = db.upsertWorktree(repo.id, "/tmp/wt1", "main", "main");
      const wt2 = db.upsertWorktree(repo.id, "/tmp/wt2", "feature/a", "a");
      db.upsertAgentStatus(wt1.id, "idle");
      db.upsertAgentStatus(wt2.id, "executing");
      const statuses = db.getAgentStatuses(repo.id);
      expect(statuses.size).toBe(2);
      expect(statuses.get(wt1.id)!.status).toBe("idle");
      expect(statuses.get(wt2.id)!.status).toBe("executing");
    });

    it("getAllAgentStatuses returns all statuses", () => {
      const repo = db.addRepository("/tmp/repo", "repo");
      const wt = db.upsertWorktree(repo.id, "/tmp/wt", "main", "main");
      db.upsertAgentStatus(wt.id, "idle");
      const all = db.getAllAgentStatuses();
      expect(all.size).toBe(1);
    });

    it("returns undefined for non-existent worktree status", () => {
      expect(db.getAgentStatus("non-existent")).toBeUndefined();
    });
  });

  describe("cascades", () => {
    it("removing repo cascades to worktrees", () => {
      const repo = db.addRepository("/tmp/repo", "repo");
      db.upsertWorktree(repo.id, "/tmp/wt", "main", "main");
      db.removeRepository(repo.id);
      expect(db.getAllWorktrees()).toHaveLength(0);
    });

    it("removing repo cascades to agent_status", () => {
      const repo = db.addRepository("/tmp/repo", "repo");
      const wt = db.upsertWorktree(repo.id, "/tmp/wt", "main", "main");
      db.upsertAgentStatus(wt.id, "idle");
      db.removeRepository(repo.id);
      expect(db.getAgentStatus(wt.id)).toBeUndefined();
    });

    it("removing worktree cascades to agent_status", () => {
      const repo = db.addRepository("/tmp/repo", "repo");
      const wt = db.upsertWorktree(repo.id, "/tmp/wt", "main", "main");
      db.upsertAgentStatus(wt.id, "idle");
      db.removeWorktree(wt.id);
      expect(db.getAgentStatus(wt.id)).toBeUndefined();
    });
  });

  describe("closeDb + re-open", () => {
    it("can close and reopen", () => {
      const repo = db.addRepository("/tmp/repo", "repo");
      db.closeDb();
      // Re-opening should work and data should persist
      const found = db.getRepositoryByPath("/tmp/repo");
      expect(found).toBeDefined();
      expect(found!.id).toBe(repo.id);
    });
  });

  describe("resetAll", () => {
    it("resets entire database", () => {
      db.addRepository("/tmp/repo", "repo");
      db.resetAll();
      // After reset, should start fresh
      expect(db.getRepositories()).toHaveLength(0);
    });
  });

  describe("standalone_sessions", () => {
    it("upsertStandaloneSession creates a new session", () => {
      const session = db.upsertStandaloneSession("/tmp/standalone", "executing", "sess-1", "hello", "summary", true);
      expect(session).toBeDefined();
      expect(session.path).toBe("/tmp/standalone");
      expect(session.status).toBe("executing");
      expect(session.session_id).toBe("sess-1");
      expect(session.last_response).toBe("hello");
      expect(session.transcript_summary).toBe("summary");
      expect(session.is_open).toBe(1);
    });

    it("upsertStandaloneSession updates on duplicate path", () => {
      db.upsertStandaloneSession("/tmp/standalone", "executing", "sess-1", "first", null, true);
      db.upsertStandaloneSession("/tmp/standalone", "idle", null, null, null, null);
      const session = db.getStandaloneSessionByPath("/tmp/standalone");
      expect(session).toBeDefined();
      expect(session!.status).toBe("idle");
      // COALESCE preserves previous values when null is passed
      expect(session!.session_id).toBe("sess-1");
      expect(session!.last_response).toBe("first");
    });

    it("upsertStandaloneSession defaults is_open to 1 on insert", () => {
      const session = db.upsertStandaloneSession("/tmp/standalone", "executing");
      expect(session.is_open).toBe(1);
    });

    it("getStandaloneSessions returns sessions ordered by updated_at DESC", () => {
      db.upsertStandaloneSession("/tmp/a", "idle");
      db.upsertStandaloneSession("/tmp/b", "executing");
      // Touch /tmp/a again to make it most recent
      db.upsertStandaloneSession("/tmp/a", "executing");
      const sessions = db.getStandaloneSessions();
      expect(sessions).toHaveLength(2);
      expect(sessions[0]!.path).toBe("/tmp/a");
      expect(sessions[1]!.path).toBe("/tmp/b");
    });

    it("getStandaloneSessionByPath finds by path", () => {
      db.upsertStandaloneSession("/tmp/standalone", "idle");
      const found = db.getStandaloneSessionByPath("/tmp/standalone");
      expect(found).toBeDefined();
      expect(found!.path).toBe("/tmp/standalone");
    });

    it("getStandaloneSessionByPath returns undefined for non-existent path", () => {
      expect(db.getStandaloneSessionByPath("/nope")).toBeUndefined();
    });

    it("removeStandaloneSession deletes by id", () => {
      const session = db.upsertStandaloneSession("/tmp/standalone", "idle");
      db.removeStandaloneSession(session.id);
      expect(db.getStandaloneSessionByPath("/tmp/standalone")).toBeUndefined();
    });

    it("removeStandaloneSessionByPath deletes by path", () => {
      db.upsertStandaloneSession("/tmp/standalone", "idle");
      db.removeStandaloneSessionByPath("/tmp/standalone");
      expect(db.getStandaloneSessionByPath("/tmp/standalone")).toBeUndefined();
    });

    it("pruneStaleStandaloneSessions removes old closed sessions", () => {
      // Create a closed session
      db.upsertStandaloneSession("/tmp/stale", "idle", null, null, null, false);
      // Prune with a very large maxAge so the cutoff is far in the future
      // (cutoff = now + 1 hour in the future, meaning anything before then is stale)
      const removed = db.pruneStaleStandaloneSessions(-3600000);
      expect(removed).toBe(1);
      expect(db.getStandaloneSessionByPath("/tmp/stale")).toBeUndefined();
    });

    it("pruneStaleStandaloneSessions does not remove open sessions", () => {
      db.upsertStandaloneSession("/tmp/active", "executing", null, null, null, true);
      const removed = db.pruneStaleStandaloneSessions(0);
      expect(removed).toBe(0);
      expect(db.getStandaloneSessionByPath("/tmp/active")).toBeDefined();
    });

    it("upsertWorktree removes standalone session at same path (promotion)", () => {
      db.upsertStandaloneSession("/tmp/repo/.worktrees/feat", "executing");
      const repo = db.addRepository("/tmp/repo", "repo");
      db.upsertWorktree(repo.id, "/tmp/repo/.worktrees/feat", "feature/test", "test");
      // Standalone session should be removed after promotion
      expect(db.getStandaloneSessionByPath("/tmp/repo/.worktrees/feat")).toBeUndefined();
    });
  });
});
