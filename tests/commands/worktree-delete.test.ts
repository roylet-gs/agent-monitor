import { describe, it, expect, vi, beforeEach } from "vitest";
import { captureConsole, type ConsoleSpy } from "../helpers/console-capture.js";

vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

const mockDeleteWorktree = vi.fn();
const mockDeleteBranch = vi.fn();
const mockDeleteRemoteBranch = vi.fn();
const mockRemoteBranchExists = vi.fn();

vi.mock("../../src/lib/git.js", () => ({
  isGitRepo: vi.fn(() => false),
  deleteWorktree: (...args: unknown[]) => mockDeleteWorktree(...args),
  deleteBranch: (...args: unknown[]) => mockDeleteBranch(...args),
  deleteRemoteBranch: (...args: unknown[]) => mockDeleteRemoteBranch(...args),
  remoteBranchExists: (...args: unknown[]) => mockRemoteBranchExists(...args),
}));

describe("worktree delete", () => {
  let worktreeDelete: typeof import("../../src/commands/worktree/delete.js").worktreeDelete;
  let db: typeof import("../../src/lib/db.js");
  let spy: ConsoleSpy;

  beforeEach(async () => {
    spy = captureConsole();
    mockDeleteWorktree.mockReset().mockResolvedValue(undefined);
    mockDeleteBranch.mockReset().mockResolvedValue(undefined);
    mockDeleteRemoteBranch.mockReset().mockResolvedValue(undefined);
    mockRemoteBranchExists.mockReset().mockResolvedValue(false);
    db = await import("../../src/lib/db.js");
    ({ worktreeDelete } = await import("../../src/commands/worktree/delete.js"));
  });

  it("deletes a worktree", async () => {
    const repo = db.addRepository("/tmp/repo", "repo");
    db.upsertWorktree(repo.id, "/tmp/wt", "feature/test", "test");
    await worktreeDelete("feature/test", { repo: "/tmp/repo" });
    expect(mockDeleteWorktree).toHaveBeenCalled();
    expect(db.getWorktrees(repo.id)).toHaveLength(0);
    expect(spy.getLog()).toContain("Deleted");
  });

  it("passes force flag", async () => {
    const repo = db.addRepository("/tmp/repo", "repo");
    db.upsertWorktree(repo.id, "/tmp/wt", "feature/test", "test");
    await worktreeDelete("feature/test", { repo: "/tmp/repo", force: true });
    expect(mockDeleteWorktree).toHaveBeenCalledWith("/tmp/repo", "/tmp/wt", true);
  });

  it("deletes local branch when requested", async () => {
    const repo = db.addRepository("/tmp/repo", "repo");
    db.upsertWorktree(repo.id, "/tmp/wt", "feature/test", "test");
    await worktreeDelete("feature/test", { repo: "/tmp/repo", deleteBranch: true });
    expect(mockDeleteBranch).toHaveBeenCalledWith("/tmp/repo", "feature/test", undefined);
  });

  it("deletes remote branch when requested", async () => {
    const repo = db.addRepository("/tmp/repo", "repo");
    db.upsertWorktree(repo.id, "/tmp/wt", "feature/test", "test");
    mockRemoteBranchExists.mockResolvedValue(true);
    await worktreeDelete("feature/test", { repo: "/tmp/repo", deleteRemote: true });
    expect(mockDeleteRemoteBranch).toHaveBeenCalledWith("/tmp/repo", "feature/test");
  });
});
