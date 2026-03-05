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

vi.mock("../../src/lib/github.js", () => ({
  isGhAvailable: vi.fn(() => true),
}));

// Mock the script editor opening (calls execSync)
vi.mock("../../src/lib/scripts.js", async (importOriginal) => {
  const original = await importOriginal() as any;
  return {
    ...original,
    openScriptInEditor: vi.fn(),
  };
});

describe("script commands", () => {
  let scriptShow: typeof import("../../src/commands/script.js").scriptShow;
  let scriptRemove: typeof import("../../src/commands/script.js").scriptRemove;
  let db: typeof import("../../src/lib/db.js");
  let spy: ConsoleSpy;

  beforeEach(async () => {
    spy = captureConsole();
    db = await import("../../src/lib/db.js");
    const cmds = await import("../../src/commands/script.js");
    scriptShow = cmds.scriptShow;
    scriptRemove = cmds.scriptRemove;
  });

  it("shows 'no script' when none exists", () => {
    db.addRepository("/tmp/repo", "repo");
    scriptShow({ repo: "/tmp/repo" });
    expect(spy.getLog()).toContain("No startup script");
  });

  it("removes shows 'no script' when none exists", () => {
    db.addRepository("/tmp/repo", "repo");
    scriptRemove({ repo: "/tmp/repo" });
    expect(spy.getLog()).toContain("No startup script");
  });

  it("shows script content after creation", async () => {
    const repo = db.addRepository("/tmp/repo", "repo");
    const { createStartupScript } = await import("../../src/lib/scripts.js");
    createStartupScript(repo.id);

    scriptShow({ repo: "/tmp/repo" });
    const output = spy.getLog();
    expect(output).toContain("Startup script for repo");
    expect(output).toContain("#!/");
  });

  it("removes script", async () => {
    const repo = db.addRepository("/tmp/repo", "repo");
    const { createStartupScript, hasStartupScript } = await import("../../src/lib/scripts.js");
    createStartupScript(repo.id);
    expect(hasStartupScript(repo.id)).toBe(true);

    scriptRemove({ repo: "/tmp/repo" });
    expect(spy.getLog()).toContain("Removed");
  });

  it("outputs JSON for script show", async () => {
    const repo = db.addRepository("/tmp/repo", "repo");
    const { createStartupScript } = await import("../../src/lib/scripts.js");
    createStartupScript(repo.id);

    scriptShow({ repo: "/tmp/repo", json: true });
    const parsed = JSON.parse(spy.getLog());
    expect(parsed.repo).toBe("repo");
    expect(parsed.content).toBeDefined();
  });
});
