import { describe, it, expect, beforeEach, vi } from "vitest";
import { DEFAULT_SETTINGS } from "../../src/lib/settings.js";
import type { Settings } from "../../src/lib/types.js";

vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

describe("checkWorktreeLimit", () => {
  let db: typeof import("../../src/lib/db.js");
  let checkWorktreeLimit: typeof import("../../src/lib/worktree-limit.js").checkWorktreeLimit;
  let repoId: string;

  const settings = (over: Partial<Settings>): Settings => ({
    ...DEFAULT_SETTINGS,
    ...over,
  });

  beforeEach(async () => {
    db = await import("../../src/lib/db.js");
    ({ checkWorktreeLimit } = await import("../../src/lib/worktree-limit.js"));
    repoId = db.addRepository("/tmp/limit-repo", "limit-repo").id;
  });

  const addWorktrees = (n: number, isMain = false) => {
    for (let i = 0; i < n; i++) {
      db.upsertWorktree(repoId, `/tmp/limit-repo/wt-${isMain ? "main" : i}`, `${isMain ? "main" : `feature/${i}`}`, `wt-${i}`, isMain);
    }
  };

  it("returns null when the limit is disabled, even over the cap", () => {
    addWorktrees(10);
    expect(checkWorktreeLimit(settings({ worktreeLimitEnabled: false, maxWorktrees: 5 }), repoId, "limit-repo")).toBeNull();
  });

  it("returns null when under the cap", () => {
    addWorktrees(3);
    expect(checkWorktreeLimit(settings({ worktreeLimitEnabled: true, maxWorktrees: 5 }), repoId, "limit-repo")).toBeNull();
  });

  it("returns a block message when at the cap", () => {
    addWorktrees(5);
    const msg = checkWorktreeLimit(settings({ worktreeLimitEnabled: true, maxWorktrees: 5 }), repoId, "limit-repo");
    expect(msg).toContain("5/5");
    expect(msg).toContain("limit-repo");
  });

  it("returns a block message when over the cap", () => {
    addWorktrees(7);
    expect(checkWorktreeLimit(settings({ worktreeLimitEnabled: true, maxWorktrees: 5 }), repoId, "limit-repo")).not.toBeNull();
  });

  it("excludes the main checkout from the count", () => {
    addWorktrees(4); // dedicated
    addWorktrees(1, true); // main — should not count
    // 4 dedicated < 5 cap → allowed despite 5 total rows
    expect(checkWorktreeLimit(settings({ worktreeLimitEnabled: true, maxWorktrees: 5 }), repoId, "limit-repo")).toBeNull();
  });

  it("counts per-repo, not globally", () => {
    const otherRepo = db.addRepository("/tmp/other-repo", "other-repo").id;
    for (let i = 0; i < 10; i++) {
      db.upsertWorktree(otherRepo, `/tmp/other-repo/wt-${i}`, `feature/${i}`, `wt-${i}`);
    }
    addWorktrees(2); // this repo only has 2
    expect(checkWorktreeLimit(settings({ worktreeLimitEnabled: true, maxWorktrees: 5 }), repoId, "limit-repo")).toBeNull();
  });
});
