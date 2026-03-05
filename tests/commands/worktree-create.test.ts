import { describe, it, expect, vi, beforeEach } from "vitest";
import { captureConsole, type ConsoleSpy } from "../helpers/console-capture.js";
import { ProcessExitError, mockProcessExit } from "../helpers/process-exit.js";

vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

const mockCreateWorktree = vi.fn();
const mockBranchExists = vi.fn();
const mockGetMainBranch = vi.fn();
const mockFetchBranch = vi.fn();
const mockRemoteBranchExists = vi.fn();

vi.mock("../../src/lib/git.js", () => ({
  isGitRepo: vi.fn(() => false),
  createWorktree: (...args: unknown[]) => mockCreateWorktree(...args),
  branchExists: (...args: unknown[]) => mockBranchExists(...args),
  getMainBranch: (...args: unknown[]) => mockGetMainBranch(...args),
  fetchBranch: (...args: unknown[]) => mockFetchBranch(...args),
  remoteBranchExists: (...args: unknown[]) => mockRemoteBranchExists(...args),
}));

vi.mock("../../src/lib/sync.js", () => ({
  syncWorktrees: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../../src/lib/hooks-installer.js", () => ({
  installGlobalHooks: vi.fn(),
  isGlobalHooksInstalled: vi.fn(() => true),
}));

vi.mock("../../src/lib/github.js", () => ({
  isGhAvailable: vi.fn(() => true),
}));

describe("worktree create", () => {
  let worktreeCreate: typeof import("../../src/commands/worktree/create.js").worktreeCreate;
  let db: typeof import("../../src/lib/db.js");
  let spy: ConsoleSpy;

  beforeEach(async () => {
    mockProcessExit();
    spy = captureConsole();
    mockCreateWorktree.mockReset().mockResolvedValue("/tmp/repo/.worktrees/feat");
    mockBranchExists.mockReset().mockResolvedValue(false);
    mockGetMainBranch.mockReset().mockResolvedValue("main");
    mockFetchBranch.mockReset().mockResolvedValue(undefined);
    mockRemoteBranchExists.mockReset().mockResolvedValue(false);
    db = await import("../../src/lib/db.js");
    ({ worktreeCreate } = await import("../../src/commands/worktree/create.js"));
  });

  it("creates a new worktree", async () => {
    db.addRepository("/tmp/repo", "repo");
    await worktreeCreate("feature/test", { repo: "/tmp/repo" });
    expect(mockCreateWorktree).toHaveBeenCalled();
    const args = mockCreateWorktree.mock.calls[0];
    expect(args[0]).toBe("/tmp/repo");
    expect(args[1]).toBe("feature/test");
    expect(args[2]).toBe("main"); // base branch
    expect(spy.getLog()).toContain("Created worktree");
  });

  it("outputs JSON", async () => {
    db.addRepository("/tmp/repo", "repo");
    await worktreeCreate("feature/test", { repo: "/tmp/repo", json: true });
    const parsed = JSON.parse(spy.getLog());
    expect(parsed.branch).toBe("feature/test");
    expect(parsed.path).toBe("/tmp/repo/.worktrees/feat");
  });

  it("exits if branch already exists without --reuse", async () => {
    db.addRepository("/tmp/repo", "repo");
    mockBranchExists.mockResolvedValue(true);
    await expect(
      worktreeCreate("feature/test", { repo: "/tmp/repo" })
    ).rejects.toThrow(ProcessExitError);
  });

  it("creates with --reuse when branch exists", async () => {
    db.addRepository("/tmp/repo", "repo");
    mockBranchExists.mockResolvedValue(true);
    await worktreeCreate("feature/test", { repo: "/tmp/repo", reuse: true });
    const args = mockCreateWorktree.mock.calls[0];
    expect(args[0]).toBe("/tmp/repo");
    expect(args[1]).toBe("feature/test");
    // baseBranch is still resolved (main) and passed
    expect(args[2]).toBe("main");
    expect(args[3]).toBe(true); // reuseExisting
  });

  it("uses custom base branch", async () => {
    db.addRepository("/tmp/repo", "repo");
    await worktreeCreate("feature/test", { repo: "/tmp/repo", base: "develop" });
    const args = mockCreateWorktree.mock.calls[0];
    expect(args[0]).toBe("/tmp/repo");
    expect(args[1]).toBe("feature/test");
    expect(args[2]).toBe("develop");
  });
});
