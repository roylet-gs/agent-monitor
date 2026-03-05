import { describe, it, expect, vi, beforeEach } from "vitest";
import { captureConsole, type ConsoleSpy } from "../helpers/console-capture.js";

vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

vi.mock("../../src/lib/git.js", () => ({
  isGitRepo: vi.fn(() => false),
}));

const mockSyncWorktrees = vi.fn();
vi.mock("../../src/lib/sync.js", () => ({
  syncWorktrees: (...args: unknown[]) => mockSyncWorktrees(...args),
}));

describe("worktree sync", () => {
  let worktreeSync: typeof import("../../src/commands/worktree/sync.js").worktreeSync;
  let db: typeof import("../../src/lib/db.js");
  let spy: ConsoleSpy;

  beforeEach(async () => {
    spy = captureConsole();
    mockSyncWorktrees.mockReset().mockResolvedValue(undefined);
    db = await import("../../src/lib/db.js");
    ({ worktreeSync } = await import("../../src/commands/worktree/sync.js"));
  });

  it("syncs a specific repo", async () => {
    const repo = db.addRepository("/tmp/repo", "repo");
    await worktreeSync({ repo: "/tmp/repo" });
    expect(mockSyncWorktrees).toHaveBeenCalledWith(repo.id);
    expect(spy.getLog()).toContain("Synced");
  });

  it("syncs all repos when no CWD match", async () => {
    db.addRepository("/tmp/repo1", "repo1");
    db.addRepository("/tmp/repo2", "repo2");
    await worktreeSync({});
    expect(mockSyncWorktrees).toHaveBeenCalledTimes(2);
    expect(spy.getLog()).toContain("repo1");
    expect(spy.getLog()).toContain("repo2");
  });
});
