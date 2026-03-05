import { describe, it, expect, vi, beforeEach } from "vitest";
import { captureConsole, type ConsoleSpy } from "../helpers/console-capture.js";
import { ProcessExitError, mockProcessExit } from "../helpers/process-exit.js";

vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

describe("repo remove", () => {
  let repoRemove: typeof import("../../src/commands/repo/remove.js").repoRemove;
  let db: typeof import("../../src/lib/db.js");
  let spy: ConsoleSpy;

  beforeEach(async () => {
    mockProcessExit();
    spy = captureConsole();
    db = await import("../../src/lib/db.js");
    ({ repoRemove } = await import("../../src/commands/repo/remove.js"));
  });

  it("removes repo by path", () => {
    db.addRepository("/tmp/test-repo", "test-repo");
    repoRemove("/tmp/test-repo");
    expect(db.getRepositories()).toHaveLength(0);
    expect(spy.getLog()).toContain("Removed");
  });

  it("removes repo by name", () => {
    db.addRepository("/tmp/test-repo", "test-repo");
    repoRemove("test-repo");
    expect(db.getRepositories()).toHaveLength(0);
  });

  it("exits if repo not found", () => {
    expect(() => repoRemove("nonexistent")).toThrow(ProcessExitError);
    expect(spy.getError()).toContain("not found");
  });
});
