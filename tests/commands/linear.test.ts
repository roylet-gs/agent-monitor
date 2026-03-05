import { describe, it, expect, vi, beforeEach } from "vitest";
import { captureConsole, type ConsoleSpy } from "../helpers/console-capture.js";
import { ProcessExitError, mockProcessExit } from "../helpers/process-exit.js";
import { writeFileSync, mkdirSync } from "fs";

vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

vi.mock("../../src/lib/git.js", () => ({
  isGitRepo: vi.fn(() => false),
}));

const mockFetchLinearInfo = vi.fn();
vi.mock("../../src/lib/linear.js", () => ({
  fetchLinearInfo: (...args: unknown[]) => mockFetchLinearInfo(...args),
}));

vi.mock("../../src/lib/github.js", () => ({
  isGhAvailable: vi.fn(() => true),
}));

vi.mock("open", () => ({
  default: vi.fn().mockResolvedValue(undefined),
}));

describe("linear commands", () => {
  let linearShow: typeof import("../../src/commands/linear.js").linearShow;
  let db: typeof import("../../src/lib/db.js");
  let paths: typeof import("../../src/lib/paths.js");
  let spy: ConsoleSpy;

  beforeEach(async () => {
    mockProcessExit();
    spy = captureConsole();
    mockFetchLinearInfo.mockReset();
    db = await import("../../src/lib/db.js");
    paths = await import("../../src/lib/paths.js");
    ({ linearShow } = await import("../../src/commands/linear.js"));
  });

  it("exits when Linear not configured", async () => {
    const repo = db.addRepository("/tmp/repo", "repo");
    db.upsertWorktree(repo.id, "/tmp/wt", "feature/test", "test");
    await expect(
      linearShow("feature/test", { repo: "/tmp/repo" })
    ).rejects.toThrow(ProcessExitError);
    expect(spy.getError()).toContain("not configured");
  });

  it("shows Linear ticket info", async () => {
    // Set up settings with Linear enabled
    mkdirSync(paths.APP_DIR, { recursive: true });
    writeFileSync(
      paths.SETTINGS_PATH,
      JSON.stringify({ linearEnabled: true, linearApiKey: "test-key" })
    );

    const repo = db.addRepository("/tmp/repo", "repo");
    db.upsertWorktree(repo.id, "/tmp/wt", "feature/ENG-123-my-ticket", "test");
    mockFetchLinearInfo.mockResolvedValue({
      identifier: "ENG-123",
      title: "My Ticket",
      url: "https://linear.app/team/issue/ENG-123",
      state: { name: "In Progress", color: "#f00", type: "started" },
      priorityLabel: "High",
      assignee: "Alice",
    });

    await linearShow("feature/ENG-123-my-ticket", { repo: "/tmp/repo" });
    const output = spy.getLog();
    expect(output).toContain("ENG-123");
    expect(output).toContain("My Ticket");
  });

  it("shows message when no ticket found", async () => {
    mkdirSync(paths.APP_DIR, { recursive: true });
    writeFileSync(
      paths.SETTINGS_PATH,
      JSON.stringify({ linearEnabled: true, linearApiKey: "test-key" })
    );

    const repo = db.addRepository("/tmp/repo", "repo");
    db.upsertWorktree(repo.id, "/tmp/wt", "feature/no-ticket", "test");
    mockFetchLinearInfo.mockResolvedValue(null);

    await linearShow("feature/no-ticket", { repo: "/tmp/repo" });
    expect(spy.getLog()).toContain("No Linear ticket found");
  });
});
