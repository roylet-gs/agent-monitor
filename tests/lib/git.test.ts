import { describe, it, expect, vi, beforeEach } from "vitest";

vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

// Mock simple-git
const mockRaw = vi.fn();
const mockStatus = vi.fn();
vi.mock("simple-git", () => ({
  simpleGit: () => ({ raw: mockRaw, status: mockStatus }),
}));

describe("ensureBranchForOpen", () => {
  let ensureBranchForOpen: typeof import("../../src/lib/git.js").ensureBranchForOpen;

  beforeEach(async () => {
    vi.resetModules();
    mockRaw.mockReset();
    mockStatus.mockReset();
    const git = await import("../../src/lib/git.js");
    ensureBranchForOpen = git.ensureBranchForOpen;
  });

  it("skips non-main worktrees", async () => {
    const result = await ensureBranchForOpen("/wt", "feature/foo", false);
    expect(result).toEqual({ ready: true });
    expect(mockRaw).not.toHaveBeenCalled();
  });

  it("skips when expected branch is the default branch (main)", async () => {
    // getMainBranch: rev-parse --verify main succeeds
    mockRaw.mockResolvedValueOnce("");
    const result = await ensureBranchForOpen("/repo", "main", true);
    expect(result).toEqual({ ready: true });
  });

  it("skips when already on the correct branch", async () => {
    // getMainBranch: rev-parse --verify main succeeds → "main"
    mockRaw.mockResolvedValueOnce("");
    // getCurrentBranch: rev-parse --abbrev-ref HEAD
    mockRaw.mockResolvedValueOnce("feature/foo\n");
    const result = await ensureBranchForOpen("/repo", "feature/foo", true);
    expect(result).toEqual({ ready: true });
  });

  it("errors when branch does not exist", async () => {
    // getMainBranch → "main"
    mockRaw.mockResolvedValueOnce("");
    // getCurrentBranch → "main"
    mockRaw.mockResolvedValueOnce("main\n");
    // branchExists: local check fails
    mockRaw.mockRejectedValueOnce(new Error("not found"));
    // branchExists: remote check fails
    mockRaw.mockRejectedValueOnce(new Error("not found"));

    const result = await ensureBranchForOpen("/repo", "feature/gone", true);
    expect(result.ready).toBe(false);
    expect(result.error).toContain("does not exist");
  });

  it("errors when working tree is dirty", async () => {
    // getMainBranch → "main"
    mockRaw.mockResolvedValueOnce("");
    // getCurrentBranch → "main"
    mockRaw.mockResolvedValueOnce("main\n");
    // branchExists: local check succeeds
    mockRaw.mockResolvedValueOnce("");
    // getGitStatus: status() call
    mockStatus.mockResolvedValueOnce({ files: [{ path: "a.ts" }, { path: "b.ts" }] });
    // getGitStatus: rev-list for ahead/behind
    mockRaw.mockResolvedValueOnce("0\t0");

    const result = await ensureBranchForOpen("/repo", "feature/dirty", true);
    expect(result.ready).toBe(false);
    expect(result.error).toContain("2 uncommitted changes");
    expect(result.error).toContain("Stash or commit first");
  });

  it("checks out branch and returns switched on clean tree", async () => {
    // getMainBranch → "main"
    mockRaw.mockResolvedValueOnce("");
    // getCurrentBranch → "main"
    mockRaw.mockResolvedValueOnce("main\n");
    // branchExists: local check succeeds
    mockRaw.mockResolvedValueOnce("");
    // getGitStatus: status()
    mockStatus.mockResolvedValueOnce({ files: [] });
    // getGitStatus: rev-list
    mockRaw.mockResolvedValueOnce("0\t0");
    // checkoutBranch
    mockRaw.mockResolvedValueOnce("");

    const result = await ensureBranchForOpen("/repo", "feature/clean", true);
    expect(result).toEqual({ ready: true, switched: true });
    // Verify checkout was called with correct args
    expect(mockRaw).toHaveBeenCalledWith(["checkout", "feature/clean"]);
  });

  it("shows singular 'change' for 1 dirty file", async () => {
    // getMainBranch → "main"
    mockRaw.mockResolvedValueOnce("");
    // getCurrentBranch → "main"
    mockRaw.mockResolvedValueOnce("main\n");
    // branchExists: local check succeeds
    mockRaw.mockResolvedValueOnce("");
    // getGitStatus: status()
    mockStatus.mockResolvedValueOnce({ files: [{ path: "a.ts" }] });
    // getGitStatus: rev-list
    mockRaw.mockResolvedValueOnce("0\t0");

    const result = await ensureBranchForOpen("/repo", "feature/one", true);
    expect(result.ready).toBe(false);
    expect(result.error).toContain("1 uncommitted change.");
    expect(result.error).not.toContain("changes.");
  });
});
