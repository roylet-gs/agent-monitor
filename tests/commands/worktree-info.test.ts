import { describe, it, expect, vi, beforeEach } from "vitest";
import { captureConsole, type ConsoleSpy } from "../helpers/console-capture.js";

vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

vi.mock("../../src/lib/git.js", () => ({
  isGitRepo: vi.fn(() => false),
  getGitStatus: vi.fn().mockResolvedValue({ ahead: 1, behind: 0, dirty: 3 }),
  getLastCommit: vi.fn().mockResolvedValue({ hash: "abc1234", message: "test commit", relative_time: "5m ago" }),
}));

vi.mock("../../src/lib/github.js", () => ({
  fetchPrInfo: vi.fn().mockResolvedValue(null),
  getPrStatusLabel: vi.fn(() => ({ label: "In Review", color: "cyan" })),
  isGhAvailable: vi.fn(() => true),
}));

vi.mock("../../src/lib/linear.js", () => ({
  fetchLinearInfo: vi.fn().mockResolvedValue(null),
}));

describe("worktree info", () => {
  let worktreeInfo: typeof import("../../src/commands/worktree/info.js").worktreeInfo;
  let db: typeof import("../../src/lib/db.js");
  let spy: ConsoleSpy;

  beforeEach(async () => {
    spy = captureConsole();
    db = await import("../../src/lib/db.js");
    ({ worktreeInfo } = await import("../../src/commands/worktree/info.js"));
  });

  it("shows worktree info", async () => {
    const repo = db.addRepository("/tmp/repo", "repo");
    db.upsertWorktree(repo.id, "/tmp/wt", "feature/test", "test");
    await worktreeInfo("feature/test", { repo: "/tmp/repo" });
    const output = spy.getLog();
    expect(output).toContain("feature/test");
    expect(output).toContain("repo");
  });

  it("outputs JSON", async () => {
    const repo = db.addRepository("/tmp/repo", "repo");
    db.upsertWorktree(repo.id, "/tmp/wt", "feature/test", "test");
    await worktreeInfo("feature/test", { repo: "/tmp/repo", json: true });
    const parsed = JSON.parse(spy.getLog());
    expect(parsed.branch).toBe("feature/test");
    expect(parsed.git).toBeDefined();
  });

  it("includes agent status when set", async () => {
    const repo = db.addRepository("/tmp/repo", "repo");
    const wt = db.upsertWorktree(repo.id, "/tmp/wt", "feature/test", "test");
    db.upsertAgentStatus(wt.id, "executing", "sess-1");
    await worktreeInfo("feature/test", { repo: "/tmp/repo" });
    expect(spy.getLog()).toContain("executing");
  });
});
