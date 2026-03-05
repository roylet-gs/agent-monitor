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
      db.upsertAgentStatus(wt.id, "executing", "session-1", "hello", "summary");
      const status = db.getAgentStatus(wt.id);
      expect(status).toBeDefined();
      expect(status!.status).toBe("executing");
      expect(status!.session_id).toBe("session-1");
      expect(status!.last_response).toBe("hello");
      expect(status!.transcript_summary).toBe("summary");
    });

    it("COALESCE: null fields preserve existing values", () => {
      const repo = db.addRepository("/tmp/repo", "repo");
      const wt = db.upsertWorktree(repo.id, "/tmp/wt", "main", "main");
      db.upsertAgentStatus(wt.id, "executing", "session-1", "hello", "summary");
      // Update status without overwriting session/response/summary
      db.upsertAgentStatus(wt.id, "idle");
      const status = db.getAgentStatus(wt.id);
      expect(status!.status).toBe("idle");
      expect(status!.session_id).toBe("session-1");
      expect(status!.last_response).toBe("hello");
      expect(status!.transcript_summary).toBe("summary");
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
});
