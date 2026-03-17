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

const mockExecSync = vi.fn();
vi.mock("child_process", () => ({
  execSync: (...args: unknown[]) => mockExecSync(...args),
}));

const mockExistsSync = vi.fn();
const mockReadFileSync = vi.fn();
const mockRmSync = vi.fn();
vi.mock("fs", () => ({
  existsSync: (...args: unknown[]) => mockExistsSync(...args),
  readFileSync: (...args: unknown[]) => mockReadFileSync(...args),
  rmSync: (...args: unknown[]) => mockRmSync(...args),
}));

describe("listWorktrees", () => {
  let listWorktrees: typeof import("../../src/lib/git.js").listWorktrees;

  beforeEach(async () => {
    vi.resetModules();
    mockRaw.mockReset();
    mockExecSync.mockReset();
    mockExistsSync.mockReset();
    mockReadFileSync.mockReset();
    const git = await import("../../src/lib/git.js");
    listWorktrees = git.listWorktrees;
  });

  it("parses normal porcelain output", async () => {
    mockRaw.mockResolvedValueOnce(
      "worktree /tmp/repo\nbranch refs/heads/main\n\nworktree /tmp/repo/.wt/feat\nbranch refs/heads/feature/foo\n\n"
    );

    const result = await listWorktrees("/tmp/repo");
    expect(result).toEqual([
      { path: "/tmp/repo", branch: "main", isMain: true },
      { path: "/tmp/repo/.wt/feat", branch: "feature/foo", isMain: false },
    ]);
  });

  it("recovers branch for detached worktree mid-rebase", async () => {
    mockRaw.mockResolvedValueOnce(
      "worktree /tmp/repo\nbranch refs/heads/main\n\nworktree /tmp/repo/.wt/feat\ndetached\n\n"
    );

    // recoverDetachedBranch: git rev-parse --git-dir
    mockExecSync.mockReturnValueOnce("/tmp/repo/.wt/feat/.git\n");
    // existsSync for rebase-merge/head-name
    mockExistsSync.mockReturnValueOnce(true);
    // readFileSync for head-name
    mockReadFileSync.mockReturnValueOnce("refs/heads/feature/foo\n");

    const result = await listWorktrees("/tmp/repo");
    expect(result).toHaveLength(2);
    expect(result[1]).toEqual({
      path: "/tmp/repo/.wt/feat",
      branch: "feature/foo",
      isMain: false,
    });
  });

  it("recovers branch from rebase-apply when rebase-merge missing", async () => {
    mockRaw.mockResolvedValueOnce(
      "worktree /tmp/repo/.wt/feat\ndetached\n\n"
    );

    mockExecSync.mockReturnValueOnce(".git\n");
    // rebase-merge/head-name does not exist
    mockExistsSync.mockReturnValueOnce(false);
    // rebase-apply/head-name exists
    mockExistsSync.mockReturnValueOnce(true);
    mockReadFileSync.mockReturnValueOnce("refs/heads/feature/bar\n");

    const result = await listWorktrees("/tmp/repo");
    expect(result).toHaveLength(1);
    expect(result[0]!.branch).toBe("feature/bar");
  });

  it("does not lose entries when blank line separators are missing", async () => {
    mockRaw.mockResolvedValueOnce(
      "worktree /tmp/repo\nbranch refs/heads/main\nworktree /tmp/repo/.wt/feat\nbranch refs/heads/feature/foo\n"
    );

    const result = await listWorktrees("/tmp/repo");
    expect(result).toEqual([
      { path: "/tmp/repo", branch: "main", isMain: true },
      { path: "/tmp/repo/.wt/feat", branch: "feature/foo", isMain: false },
    ]);
  });

  it("skips detached worktree when recovery fails", async () => {
    mockRaw.mockResolvedValueOnce(
      "worktree /tmp/repo\nbranch refs/heads/main\n\nworktree /tmp/repo/.wt/feat\ndetached\n\n"
    );

    // recoverDetachedBranch: execSync throws
    mockExecSync.mockImplementationOnce(() => { throw new Error("not a git dir"); });

    const result = await listWorktrees("/tmp/repo");
    expect(result).toHaveLength(1);
    expect(result[0]!.branch).toBe("main");
  });
});

describe("recoverDetachedBranch", () => {
  let recoverDetachedBranch: typeof import("../../src/lib/git.js").recoverDetachedBranch;

  beforeEach(async () => {
    vi.resetModules();
    mockExecSync.mockReset();
    mockExistsSync.mockReset();
    mockReadFileSync.mockReset();
    const git = await import("../../src/lib/git.js");
    recoverDetachedBranch = git.recoverDetachedBranch;
  });

  it("returns undefined when not in rebase", () => {
    mockExecSync.mockReturnValueOnce(".git\n");
    mockExistsSync.mockReturnValue(false);

    expect(recoverDetachedBranch("/tmp/wt")).toBeUndefined();
  });

  it("returns undefined when git dir resolution fails", () => {
    mockExecSync.mockImplementationOnce(() => { throw new Error("fail"); });
    expect(recoverDetachedBranch("/tmp/wt")).toBeUndefined();
  });
});

describe("ensureBranchForOpen", () => {
  let ensureBranchForOpen: typeof import("../../src/lib/git.js").ensureBranchForOpen;

  beforeEach(async () => {
    vi.resetModules();
    mockRaw.mockReset();
    mockStatus.mockReset();
    mockExecSync.mockReset();
    mockExistsSync.mockReset();
    mockReadFileSync.mockReset();
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

describe("deleteWorktree", () => {
  let deleteWorktree: typeof import("../../src/lib/git.js").deleteWorktree;

  beforeEach(async () => {
    vi.resetModules();
    mockRaw.mockReset();
    mockExistsSync.mockReset();
    mockRmSync.mockReset();
    const git = await import("../../src/lib/git.js");
    deleteWorktree = git.deleteWorktree;
  });

  it("calls git worktree remove", async () => {
    mockRaw.mockResolvedValueOnce("");
    await deleteWorktree("/repo", "/repo/.wt/feat");
    expect(mockRaw).toHaveBeenCalledWith(["worktree", "remove", "/repo/.wt/feat"]);
  });

  it("adds --force flag when force=true", async () => {
    mockRaw.mockResolvedValueOnce("");
    await deleteWorktree("/repo", "/repo/.wt/feat", true);
    expect(mockRaw).toHaveBeenCalledWith(["worktree", "remove", "/repo/.wt/feat", "--force"]);
  });

  it("falls back to rmSync + prune when force removal fails with directory not empty", async () => {
    mockRaw.mockRejectedValueOnce(new Error("failed to delete: Directory not empty"));
    mockExistsSync.mockReturnValueOnce(true);
    mockRmSync.mockReturnValueOnce(undefined);
    mockRaw.mockResolvedValueOnce(""); // git worktree prune

    await deleteWorktree("/repo", "/repo/.wt/feat", true);

    expect(mockRmSync).toHaveBeenCalledWith("/repo/.wt/feat", { recursive: true, force: true });
    expect(mockRaw).toHaveBeenCalledWith(["worktree", "prune"]);
  });

  it("re-throws error when force=false", async () => {
    mockRaw.mockRejectedValueOnce(new Error("Directory not empty"));
    await expect(deleteWorktree("/repo", "/repo/.wt/feat", false)).rejects.toThrow("Directory not empty");
    expect(mockRmSync).not.toHaveBeenCalled();
  });

  it("re-throws error when path no longer exists on disk", async () => {
    mockRaw.mockRejectedValueOnce(new Error("Directory not empty"));
    mockExistsSync.mockReturnValueOnce(false);
    await expect(deleteWorktree("/repo", "/repo/.wt/feat", true)).rejects.toThrow("Directory not empty");
    expect(mockRmSync).not.toHaveBeenCalled();
  });
});
