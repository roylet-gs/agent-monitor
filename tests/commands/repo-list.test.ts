import { describe, it, expect, vi, beforeEach } from "vitest";
import { captureConsole, type ConsoleSpy } from "../helpers/console-capture.js";

vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

describe("repo list", () => {
  let repoList: typeof import("../../src/commands/repo/list.js").repoList;
  let db: typeof import("../../src/lib/db.js");
  let spy: ConsoleSpy;

  beforeEach(async () => {
    spy = captureConsole();
    db = await import("../../src/lib/db.js");
    ({ repoList } = await import("../../src/commands/repo/list.js"));
  });

  it("shows help when no repos", () => {
    repoList({});
    expect(spy.getLog()).toContain("No repositories");
  });

  it("lists repos as table", () => {
    db.addRepository("/tmp/repo1", "repo1");
    db.addRepository("/tmp/repo2", "repo2");
    repoList({});
    const output = spy.getLog();
    expect(output).toContain("repo1");
    expect(output).toContain("repo2");
  });

  it("outputs JSON when --json flag is set", () => {
    db.addRepository("/tmp/repo1", "repo1");
    repoList({ json: true });
    const parsed = JSON.parse(spy.getLog());
    expect(Array.isArray(parsed)).toBe(true);
    expect(parsed[0].name).toBe("repo1");
  });
});
