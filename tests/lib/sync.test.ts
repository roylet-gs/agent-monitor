import { describe, it, expect, vi, beforeEach } from "vitest";

vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

const mockListWorktrees = vi.fn();
const mockGetRepoName = vi.fn();

vi.mock("../../src/lib/git.js", () => ({
  listWorktrees: (...args: unknown[]) => mockListWorktrees(...args),
  getRepoName: (...args: unknown[]) => mockGetRepoName(...args),
}));

const mockExistsSyncForSync = vi.fn();
vi.mock("fs", async (importOriginal) => {
  const actual = await importOriginal<typeof import("fs")>();
  return {
    ...actual,
    existsSync: (...args: unknown[]) => {
      // Only intercept paths that contain ".worktrees/" (our test paths)
      // Let all other calls through to the real fs (e.g., db.ts mkdirSync check)
      const p = args[0] as string;
      if (typeof p === "string" && p.includes(".worktrees/")) {
        return mockExistsSyncForSync(p);
      }
      return actual.existsSync(p);
    },
  };
});

describe("syncWorktrees", () => {
  let sync: typeof import("../../src/lib/sync.js");
  let db: typeof import("../../src/lib/db.js");

  beforeEach(async () => {
    mockListWorktrees.mockReset();
    mockGetRepoName.mockReset();
    mockExistsSyncForSync.mockReset();
    mockExistsSyncForSync.mockReturnValue(false);
    db = await import("../../src/lib/db.js");
    sync = await import("../../src/lib/sync.js");
  });

  it("adds new worktrees from git to DB including main", async () => {
    const repo = db.addRepository("/tmp/repo", "repo");
    mockListWorktrees.mockResolvedValue([
      { path: "/tmp/repo", branch: "main", isMain: true },
      { path: "/tmp/repo/.worktrees/feature-a", branch: "feature/a", isMain: false },
      { path: "/tmp/repo/.worktrees/feature-b", branch: "feature/b", isMain: false },
    ]);

    await sync.syncWorktrees(repo.id);
    const worktrees = db.getWorktrees(repo.id);
    expect(worktrees).toHaveLength(3);
    expect(worktrees.map(w => w.branch).sort()).toEqual(["feature/a", "feature/b", "main"]);
    const mainWt = worktrees.find(w => w.branch === "main");
    expect(mainWt!.is_main).toBe(1);
    expect(worktrees.find(w => w.branch === "feature/a")!.is_main).toBe(0);
  });

  it("includes main working tree in sync", async () => {
    const repo = db.addRepository("/tmp/repo", "repo");
    mockListWorktrees.mockResolvedValue([
      { path: "/tmp/repo", branch: "main", isMain: true },
    ]);

    await sync.syncWorktrees(repo.id);
    const worktrees = db.getWorktrees(repo.id);
    expect(worktrees).toHaveLength(1);
    expect(worktrees[0]!.branch).toBe("main");
    expect(worktrees[0]!.is_main).toBe(1);
  });

  it("removes DB entries for deleted git worktrees", async () => {
    const repo = db.addRepository("/tmp/repo", "repo");
    db.upsertWorktree(repo.id, "/tmp/repo/.worktrees/feature-a", "feature/a", "a");
    db.upsertWorktree(repo.id, "/tmp/repo/.worktrees/old", "feature/old", "old");

    mockListWorktrees.mockResolvedValue([
      { path: "/tmp/repo", branch: "main", isMain: true },
      { path: "/tmp/repo/.worktrees/feature-a", branch: "feature/a", isMain: false },
    ]);

    await sync.syncWorktrees(repo.id);
    const worktrees = db.getWorktrees(repo.id);
    expect(worktrees).toHaveLength(2);
    expect(worktrees.map(w => w.branch).sort()).toEqual(["feature/a", "main"]);
  });

  it("updates paths for existing worktrees", async () => {
    const repo = db.addRepository("/tmp/repo", "repo");
    db.upsertWorktree(repo.id, "/tmp/old-path", "feature/a", "a");

    mockListWorktrees.mockResolvedValue([
      { path: "/tmp/repo", branch: "main", isMain: true },
      { path: "/tmp/new-path", branch: "feature/a", isMain: false },
    ]);

    await sync.syncWorktrees(repo.id);
    const worktrees = db.getWorktrees(repo.id);
    expect(worktrees[0]!.path).toBe("/tmp/new-path");
  });

  it("keeps worktree when path exists on disk but branch missing from git", async () => {
    const repo = db.addRepository("/tmp/repo", "repo");
    db.upsertWorktree(repo.id, "/tmp/repo/.worktrees/rebasing", "feature/rebasing", "rebasing");

    mockListWorktrees.mockResolvedValue([
      { path: "/tmp/repo", branch: "main", isMain: true },
    ]);

    // The worktree path still has a .git entry (detached/rebasing)
    mockExistsSyncForSync.mockReturnValue(true);

    await sync.syncWorktrees(repo.id);
    const worktrees = db.getWorktrees(repo.id);
    expect(worktrees).toHaveLength(2);
    expect(worktrees.map(w => w.branch).sort()).toEqual(["feature/rebasing", "main"]);
  });

  it("removes worktree when path no longer exists on disk", async () => {
    const repo = db.addRepository("/tmp/repo", "repo");
    db.upsertWorktree(repo.id, "/tmp/repo/.worktrees/gone", "feature/gone", "gone");

    mockListWorktrees.mockResolvedValue([
      { path: "/tmp/repo", branch: "main", isMain: true },
    ]);

    // .git does not exist at the worktree path
    mockExistsSyncForSync.mockReturnValue(false);

    await sync.syncWorktrees(repo.id);
    const worktrees = db.getWorktrees(repo.id);
    expect(worktrees).toHaveLength(1);
    expect(worktrees[0]!.branch).toBe("main");
  });

  it("removes stale DB entry when branch is renamed (same path, different branch)", async () => {
    const repo = db.addRepository("/tmp/repo", "repo");
    db.upsertWorktree(repo.id, "/tmp/repo/.worktrees/my-feature", "feature/old-name", "old-name");

    mockListWorktrees.mockResolvedValue([
      { path: "/tmp/repo", branch: "main", isMain: true },
      { path: "/tmp/repo/.worktrees/my-feature", branch: "feature/new-name", isMain: false },
    ]);

    // Path exists on disk (same physical folder)
    mockExistsSyncForSync.mockReturnValue(true);

    await sync.syncWorktrees(repo.id);
    const worktrees = db.getWorktrees(repo.id);
    expect(worktrees).toHaveLength(2);
    expect(worktrees.map(w => w.branch).sort()).toEqual(["feature/new-name", "main"]);
  });

  it("does nothing for non-existent repo", async () => {
    await sync.syncWorktrees("non-existent-id");
    expect(mockListWorktrees).not.toHaveBeenCalled();
  });
});
