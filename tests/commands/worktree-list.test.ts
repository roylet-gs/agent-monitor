import { describe, it, expect, vi, beforeEach } from "vitest";
import { captureConsole, type ConsoleSpy } from "../helpers/console-capture.js";
import { mkdtempSync } from "fs";
import { tmpdir } from "os";
import { join } from "path";

vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

vi.mock("../../src/lib/git.js", () => ({
  isGitRepo: vi.fn(() => false),
  getGitStatus: vi.fn().mockResolvedValue({ ahead: 0, behind: 0, dirty: 0 }),
  getLastCommit: vi.fn().mockResolvedValue({ hash: "abc1234", message: "test", relative_time: "1m ago" }),
}));

vi.mock("../../src/lib/github.js", () => ({
  fetchPrInfo: vi.fn().mockResolvedValue(null),
  getPrStatusLabel: vi.fn(() => ({ label: "In Review", color: "cyan" })),
  isGhAvailable: vi.fn(() => true),
}));

vi.mock("../../src/lib/linear.js", () => ({
  fetchLinearInfo: vi.fn().mockResolvedValue(null),
}));

describe("worktree list", () => {
  let worktreeList: typeof import("../../src/commands/worktree/list.js").worktreeList;
  let db: typeof import("../../src/lib/db.js");
  let spy: ConsoleSpy;
  let tempWtDir: string;

  beforeEach(async () => {
    spy = captureConsole();
    // Create a real temp dir so existsSync returns true
    tempWtDir = mkdtempSync(join(tmpdir(), "am-wt-list-"));
    db = await import("../../src/lib/db.js");
    ({ worktreeList } = await import("../../src/commands/worktree/list.js"));
  });

  it("shows message when no worktrees", async () => {
    db.addRepository("/tmp/repo", "repo");
    await worktreeList({ repo: "/tmp/repo" });
    const output = spy.getLog();
    expect(output).toBeTruthy();
  });

  it("lists worktrees as table", async () => {
    const repo = db.addRepository("/tmp/repo", "repo");
    db.upsertWorktree(repo.id, tempWtDir, "feature/test", "test");
    await worktreeList({ repo: "/tmp/repo" });
    const output = spy.getLog();
    expect(output).toContain("feature/test");
  });

  it("outputs JSON when --json flag is set", async () => {
    const repo = db.addRepository("/tmp/repo", "repo");
    db.upsertWorktree(repo.id, tempWtDir, "feature/test", "test");
    await worktreeList({ repo: "/tmp/repo", json: true });
    const parsed = JSON.parse(spy.getLog());
    expect(Array.isArray(parsed)).toBe(true);
    expect(parsed[0].branch).toBe("feature/test");
  });
});
