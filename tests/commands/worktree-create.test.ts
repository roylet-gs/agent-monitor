import { describe, it, expect, vi, beforeEach } from "vitest";
import { captureConsole, type ConsoleSpy } from "../helpers/console-capture.js";
import { ProcessExitError, mockProcessExit } from "../helpers/process-exit.js";

vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

const mockCreateWorktree = vi.fn();
const mockLocalBranchExists = vi.fn();
const mockLsRemoteBranch = vi.fn();
const mockGetMainBranch = vi.fn();
const mockFetchBranch = vi.fn();
const mockFetchAndResetBranch = vi.fn();
const mockRemoteBranchExists = vi.fn();
const mockDeleteBranch = vi.fn();

vi.mock("../../src/lib/git.js", () => ({
  isGitRepo: vi.fn(() => false),
  createWorktree: (...args: unknown[]) => mockCreateWorktree(...args),
  localBranchExists: (...args: unknown[]) => mockLocalBranchExists(...args),
  lsRemoteBranch: (...args: unknown[]) => mockLsRemoteBranch(...args),
  getMainBranch: (...args: unknown[]) => mockGetMainBranch(...args),
  fetchBranch: (...args: unknown[]) => mockFetchBranch(...args),
  fetchAndResetBranch: (...args: unknown[]) => mockFetchAndResetBranch(...args),
  remoteBranchExists: (...args: unknown[]) => mockRemoteBranchExists(...args),
  deleteBranch: (...args: unknown[]) => mockDeleteBranch(...args),
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
    mockLocalBranchExists.mockReset().mockResolvedValue(false);
    mockLsRemoteBranch.mockReset().mockResolvedValue(false);
    mockGetMainBranch.mockReset().mockResolvedValue("main");
    mockFetchBranch.mockReset().mockResolvedValue(undefined);
    mockFetchAndResetBranch.mockReset().mockResolvedValue(true);
    mockRemoteBranchExists.mockReset().mockResolvedValue(false);
    mockDeleteBranch.mockReset().mockResolvedValue(undefined);
    db = await import("../../src/lib/db.js");
    ({ worktreeCreate } = await import("../../src/commands/worktree/create.js"));
  });

  it("creates a fresh worktree with no flags", async () => {
    db.addRepository("/tmp/repo", "repo");
    await worktreeCreate("feature/test", { repo: "/tmp/repo" });
    expect(mockCreateWorktree).toHaveBeenCalled();
    const args = mockCreateWorktree.mock.calls[0];
    expect(args[0]).toBe("/tmp/repo");
    expect(args[1]).toBe("feature/test");
    expect(args[2]).toEqual({ baseRef: "main" });
    expect(spy.getLog()).toContain("Created worktree");
  });

  it("outputs JSON with mode", async () => {
    db.addRepository("/tmp/repo", "repo");
    await worktreeCreate("feature/test", { repo: "/tmp/repo", json: true });
    const parsed = JSON.parse(spy.getLog());
    expect(parsed.branch).toBe("feature/test");
    expect(parsed.path).toBe("/tmp/repo/.worktrees/feat");
    expect(parsed.mode).toBe("fresh");
  });

  it("exits when branch exists locally and no flag is set", async () => {
    db.addRepository("/tmp/repo", "repo");
    mockLocalBranchExists.mockResolvedValue(true);
    await expect(
      worktreeCreate("feature/test", { repo: "/tmp/repo" })
    ).rejects.toThrow(ProcessExitError);
    expect(spy.getError()).toContain("locally");
    expect(spy.getError()).toContain("--reuse");
  });

  it("exits when branch exists remotely with hint about --track / --no-track", async () => {
    db.addRepository("/tmp/repo", "repo");
    mockLsRemoteBranch.mockResolvedValue(true);
    await expect(
      worktreeCreate("feature/test", { repo: "/tmp/repo" })
    ).rejects.toThrow(ProcessExitError);
    expect(spy.getError()).toContain("on origin");
    expect(spy.getError()).toContain("--track");
    expect(spy.getError()).toContain("--no-track");
  });

  it("--reuse attaches to existing local branch via positional reuse", async () => {
    db.addRepository("/tmp/repo", "repo");
    mockLocalBranchExists.mockResolvedValue(true);
    await worktreeCreate("feature/test", { repo: "/tmp/repo", reuse: true });
    const args = mockCreateWorktree.mock.calls[0];
    expect(args[2]).toEqual({ reuse: true });
    expect(mockFetchAndResetBranch).toHaveBeenCalledWith("/tmp/repo", "feature/test");
  });

  it("--track pulls remote branch", async () => {
    db.addRepository("/tmp/repo", "repo");
    mockLsRemoteBranch.mockResolvedValue(true);
    await worktreeCreate("feature/test", { repo: "/tmp/repo", track: true });
    const args = mockCreateWorktree.mock.calls[0];
    expect(args[2]).toEqual({ reuse: true });
    expect(mockFetchAndResetBranch).toHaveBeenCalledWith("/tmp/repo", "feature/test");
  });

  it("--track errors when remote branch is absent", async () => {
    db.addRepository("/tmp/repo", "repo");
    mockLsRemoteBranch.mockResolvedValue(false);
    await expect(
      worktreeCreate("feature/test", { repo: "/tmp/repo", track: true })
    ).rejects.toThrow(ProcessExitError);
    expect(spy.getError()).toContain("does not exist on origin");
  });

  it("--no-track creates a disconnected branch from base, deleting local first if needed", async () => {
    db.addRepository("/tmp/repo", "repo");
    mockLocalBranchExists.mockResolvedValue(true);
    mockLsRemoteBranch.mockResolvedValue(true);
    mockRemoteBranchExists.mockResolvedValue(true); // base branch has a remote tracking ref
    await worktreeCreate("feature/test", { repo: "/tmp/repo", noTrack: true });
    expect(mockDeleteBranch).toHaveBeenCalledWith("/tmp/repo", "feature/test", true);
    const args = mockCreateWorktree.mock.calls[0];
    expect(args[2]).toEqual({ baseRef: "origin/main", noTrack: true });
  });

  it("--reuse and --track are mutually exclusive", async () => {
    db.addRepository("/tmp/repo", "repo");
    await expect(
      worktreeCreate("feature/test", { repo: "/tmp/repo", reuse: true, track: true })
    ).rejects.toThrow(ProcessExitError);
  });

  it("uses custom base branch", async () => {
    db.addRepository("/tmp/repo", "repo");
    await worktreeCreate("feature/test", { repo: "/tmp/repo", base: "develop" });
    const args = mockCreateWorktree.mock.calls[0];
    expect(args[1]).toBe("feature/test");
    expect(args[2]).toEqual({ baseRef: "develop" });
  });
});
