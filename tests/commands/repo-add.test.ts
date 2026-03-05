import { describe, it, expect, vi, beforeEach } from "vitest";
import { captureConsole, type ConsoleSpy } from "../helpers/console-capture.js";
import { ProcessExitError, mockProcessExit } from "../helpers/process-exit.js";
import { createTempGitRepo } from "../helpers/test-git-repo.js";

vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

// Mock syncWorktrees to avoid real git operations in sync
const mockSyncWorktrees = vi.fn().mockResolvedValue(undefined);
vi.mock("../../src/lib/sync.js", () => ({
  syncWorktrees: (...args: unknown[]) => mockSyncWorktrees(...args),
}));

describe("repo add", () => {
  let repoAdd: typeof import("../../src/commands/repo/add.js").repoAdd;
  let db: typeof import("../../src/lib/db.js");
  let spy: ConsoleSpy;

  beforeEach(async () => {
    mockProcessExit();
    spy = captureConsole();
    mockSyncWorktrees.mockReset().mockResolvedValue(undefined);
    db = await import("../../src/lib/db.js");
    ({ repoAdd } = await import("../../src/commands/repo/add.js"));
  });

  it("adds a git repository", async () => {
    const repoPath = createTempGitRepo();
    await repoAdd(repoPath, {});
    const repos = db.getRepositories();
    expect(repos).toHaveLength(1);
    expect(spy.getLog()).toContain("Added repository");
  });

  it("outputs JSON when --json flag is set", async () => {
    const repoPath = createTempGitRepo();
    await repoAdd(repoPath, { json: true });
    const parsed = JSON.parse(spy.getLog());
    expect(parsed.path).toBe(repoPath);
  });

  it("syncs worktrees after adding", async () => {
    const repoPath = createTempGitRepo();
    await repoAdd(repoPath, {});
    expect(mockSyncWorktrees).toHaveBeenCalled();
  });

  it("exits if path is not a git repo", async () => {
    await expect(repoAdd("/tmp/not-a-repo", {})).rejects.toThrow(ProcessExitError);
    expect(spy.getError()).toContain("Not a git repository");
  });

  it("notifies if repo already tracked", async () => {
    const repoPath = createTempGitRepo();
    await repoAdd(repoPath, {});
    spy = captureConsole(); // reset spy
    await repoAdd(repoPath, {});
    expect(spy.getLog()).toContain("already tracked");
  });
});
