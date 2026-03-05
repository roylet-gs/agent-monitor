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

describe("syncWorktrees", () => {
  let sync: typeof import("../../src/lib/sync.js");
  let db: typeof import("../../src/lib/db.js");

  beforeEach(async () => {
    mockListWorktrees.mockReset();
    mockGetRepoName.mockReset();
    db = await import("../../src/lib/db.js");
    sync = await import("../../src/lib/sync.js");
  });

  it("adds new worktrees from git to DB", async () => {
    const repo = db.addRepository("/tmp/repo", "repo");
    mockListWorktrees.mockResolvedValue([
      { path: "/tmp/repo", branch: "main", isMain: true },
      { path: "/tmp/repo/.worktrees/feature-a", branch: "feature/a", isMain: false },
    ]);

    await sync.syncWorktrees(repo.id);
    const worktrees = db.getWorktrees(repo.id);
    expect(worktrees).toHaveLength(2);
  });

  it("removes DB entries for deleted git worktrees", async () => {
    const repo = db.addRepository("/tmp/repo", "repo");
    db.upsertWorktree(repo.id, "/tmp/repo", "main", "main");
    db.upsertWorktree(repo.id, "/tmp/repo/.worktrees/old", "feature/old", "old");

    mockListWorktrees.mockResolvedValue([
      { path: "/tmp/repo", branch: "main", isMain: true },
    ]);

    await sync.syncWorktrees(repo.id);
    const worktrees = db.getWorktrees(repo.id);
    expect(worktrees).toHaveLength(1);
    expect(worktrees[0]!.branch).toBe("main");
  });

  it("updates paths for existing worktrees", async () => {
    const repo = db.addRepository("/tmp/repo", "repo");
    db.upsertWorktree(repo.id, "/tmp/old-path", "main", "main");

    mockListWorktrees.mockResolvedValue([
      { path: "/tmp/new-path", branch: "main", isMain: true },
    ]);

    await sync.syncWorktrees(repo.id);
    const worktrees = db.getWorktrees(repo.id);
    expect(worktrees[0]!.path).toBe("/tmp/new-path");
  });

  it("does nothing for non-existent repo", async () => {
    await sync.syncWorktrees("non-existent-id");
    expect(mockListWorktrees).not.toHaveBeenCalled();
  });
});
