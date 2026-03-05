import { describe, it, expect, vi, beforeEach } from "vitest";
import { ProcessExitError, mockProcessExit } from "../helpers/process-exit.js";
import { captureConsole, type ConsoleSpy } from "../helpers/console-capture.js";

vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

vi.mock("../../src/lib/git.js", () => ({
  isGitRepo: vi.fn(() => false),
}));

describe("resolve", () => {
  let resolve: typeof import("../../src/lib/resolve.js");
  let db: typeof import("../../src/lib/db.js");
  let spy: ConsoleSpy;

  beforeEach(async () => {
    mockProcessExit();
    spy = captureConsole();
    db = await import("../../src/lib/db.js");
    resolve = await import("../../src/lib/resolve.js");
  });

  describe("resolveRepo", () => {
    it("resolves by explicit path", () => {
      db.addRepository("/tmp/my-repo", "my-repo");
      const repo = resolve.resolveRepo("/tmp/my-repo");
      expect(repo.name).toBe("my-repo");
    });

    it("exits if explicit path not tracked", () => {
      expect(() => resolve.resolveRepo("/tmp/not-tracked")).toThrow(ProcessExitError);
      expect(spy.getError()).toContain("not tracked");
    });

    it("exits if CWD detection fails", () => {
      expect(() => resolve.resolveRepo()).toThrow(ProcessExitError);
      expect(spy.getError()).toContain("Could not detect");
    });
  });

  describe("resolveWorktree", () => {
    it("resolves by exact branch name", () => {
      const repo = db.addRepository("/tmp/repo", "repo");
      db.upsertWorktree(repo.id, "/tmp/wt", "feature/test", "test");
      const wt = resolve.resolveWorktree("feature/test", repo.id);
      expect(wt.branch).toBe("feature/test");
    });

    it("resolves by partial branch name", () => {
      const repo = db.addRepository("/tmp/repo", "repo");
      db.upsertWorktree(repo.id, "/tmp/wt", "feature/my-thing", "my-thing");
      const wt = resolve.resolveWorktree("my-thing", repo.id);
      expect(wt.branch).toBe("feature/my-thing");
    });

    it("resolves by worktree name", () => {
      const repo = db.addRepository("/tmp/repo", "repo");
      db.upsertWorktree(repo.id, "/tmp/wt", "feature/test", "test");
      const wt = resolve.resolveWorktree("test", repo.id);
      expect(wt.branch).toBe("feature/test");
    });

    it("exits on ambiguous branch match", () => {
      const repo = db.addRepository("/tmp/repo", "repo");
      db.upsertWorktree(repo.id, "/tmp/wt1", "feature/test", "test1");
      db.upsertWorktree(repo.id, "/tmp/wt2", "bugfix/test", "test2");
      // Both end with /test
      expect(() => resolve.resolveWorktree("test", repo.id)).toThrow(ProcessExitError);
      expect(spy.getError()).toContain("Ambiguous");
    });

    it("exits when not found", () => {
      const repo = db.addRepository("/tmp/repo", "repo");
      expect(() => resolve.resolveWorktree("nonexistent", repo.id)).toThrow(ProcessExitError);
      expect(spy.getError()).toContain("not found");
    });

    it("searches across all repos when no repoId", () => {
      const repo = db.addRepository("/tmp/repo", "repo");
      db.upsertWorktree(repo.id, "/tmp/wt", "feature/global", "global");
      const wt = resolve.resolveWorktree("feature/global");
      expect(wt.branch).toBe("feature/global");
    });
  });
});
