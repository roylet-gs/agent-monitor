import { describe, it, expect, vi, beforeEach } from "vitest";
import { captureConsole, type ConsoleSpy } from "../helpers/console-capture.js";
import { ProcessExitError, mockProcessExit } from "../helpers/process-exit.js";

vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

vi.mock("../../src/lib/pubsub-client.js", () => ({
  publishMessage: vi.fn().mockResolvedValue(undefined),
}));

describe("status command", () => {
  let printStatus: typeof import("../../src/commands/status.js").printStatus;
  let db: typeof import("../../src/lib/db.js");
  let spy: ConsoleSpy;

  beforeEach(async () => {
    mockProcessExit();
    spy = captureConsole();
    db = await import("../../src/lib/db.js");
    ({ printStatus } = await import("../../src/commands/status.js"));
  });

  it("exits with usage when no worktree path", async () => {
    await expect(printStatus()).rejects.toThrow(ProcessExitError);
    expect(spy.getLog()).toContain("Usage");
  });

  it("exits when worktree not found", async () => {
    await expect(printStatus("/tmp/nonexistent")).rejects.toThrow(ProcessExitError);
    expect(spy.getLog()).toContain("No worktree found");
  });

  it("reads current status", async () => {
    const repo = db.addRepository("/tmp/repo", "repo");
    const wt = db.upsertWorktree(repo.id, "/tmp/wt", "main", "main");
    db.upsertAgentStatus(wt.id, "executing", "sess-1");
    await printStatus("/tmp/wt");
    const output = spy.getLog();
    expect(output).toContain("executing");
    expect(output).toContain("main");
  });

  it("sets status", async () => {
    const repo = db.addRepository("/tmp/repo", "repo");
    db.upsertWorktree(repo.id, "/tmp/wt", "main", "main");
    await printStatus("/tmp/wt", "idle");
    expect(spy.getLog()).toContain('Status set to "idle"');
  });

  it("exits on invalid status", async () => {
    const repo = db.addRepository("/tmp/repo", "repo");
    db.upsertWorktree(repo.id, "/tmp/wt", "main", "main");
    await expect(printStatus("/tmp/wt", "invalid")).rejects.toThrow(ProcessExitError);
    expect(spy.getLog()).toContain("Invalid status");
  });
});
