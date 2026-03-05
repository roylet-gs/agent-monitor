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

const mockFetchPrInfo = vi.fn();
vi.mock("../../src/lib/github.js", () => ({
  fetchPrInfo: (...args: unknown[]) => mockFetchPrInfo(...args),
  getPrStatusLabel: vi.fn(() => ({ label: "In Review", color: "cyan" })),
  isGhAvailable: vi.fn(() => true),
}));

vi.mock("open", () => ({
  default: vi.fn().mockResolvedValue(undefined),
}));

describe("pr commands", () => {
  let prShow: typeof import("../../src/commands/pr.js").prShow;
  let db: typeof import("../../src/lib/db.js");
  let spy: ConsoleSpy;

  beforeEach(async () => {
    spy = captureConsole();
    mockFetchPrInfo.mockReset();
    db = await import("../../src/lib/db.js");
    ({ prShow } = await import("../../src/commands/pr.js"));
  });

  it("shows PR info for a worktree target", async () => {
    const repo = db.addRepository("/tmp/repo", "repo");
    db.upsertWorktree(repo.id, "/tmp/wt", "feature/test", "test");
    mockFetchPrInfo.mockResolvedValue({
      number: 42,
      title: "My PR",
      url: "https://github.com/test/test/pull/42",
      state: "OPEN",
      isDraft: false,
      reviewDecision: "",
      hasFeedback: false,
      checksStatus: "passing",
    });

    await prShow("feature/test", { repo: "/tmp/repo" });
    const output = spy.getLog();
    expect(output).toContain("#42");
    expect(output).toContain("My PR");
  });

  it("shows message when no PR found", async () => {
    const repo = db.addRepository("/tmp/repo", "repo");
    db.upsertWorktree(repo.id, "/tmp/wt", "feature/test", "test");
    mockFetchPrInfo.mockResolvedValue(null);

    await prShow("feature/test", { repo: "/tmp/repo" });
    expect(spy.getLog()).toContain("No PR found");
  });

  it("outputs JSON when --json flag is set", async () => {
    const repo = db.addRepository("/tmp/repo", "repo");
    db.upsertWorktree(repo.id, "/tmp/wt", "feature/test", "test");
    mockFetchPrInfo.mockResolvedValue({
      number: 42,
      title: "My PR",
      url: "https://github.com/test/test/pull/42",
      state: "OPEN",
      isDraft: false,
      reviewDecision: "",
      hasFeedback: false,
      checksStatus: "passing",
    });

    await prShow("feature/test", { repo: "/tmp/repo", json: true });
    const parsed = JSON.parse(spy.getLog());
    expect(parsed.number).toBe(42);
  });
});
