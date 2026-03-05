import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { captureConsole, type ConsoleSpy } from "../helpers/console-capture.js";
import { join } from "path";
import { tmpdir } from "os";
import { mkdtempSync, rmSync } from "fs";

vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

describe("hooks commands", () => {
  let tempHome: string;
  let originalHome: string;
  let spy: ConsoleSpy;

  beforeEach(() => {
    tempHome = mkdtempSync(join(tmpdir(), "am-hooks-cmd-"));
    originalHome = process.env.HOME!;
    process.env.HOME = tempHome;
    spy = captureConsole();
  });

  afterEach(() => {
    process.env.HOME = originalHome;
    rmSync(tempHome, { recursive: true, force: true });
  });

  it("hooksInstall logs success", async () => {
    const { hooksInstall } = await import("../../src/commands/hooks.js");
    hooksInstall();
    expect(spy.getLog()).toContain("Hooks installed");
  });

  it("hooksUninstall logs success", async () => {
    const { hooksInstall, hooksUninstall } = await import("../../src/commands/hooks.js");
    hooksInstall();
    spy = captureConsole();
    hooksUninstall();
    expect(spy.getLog()).toContain("Hooks removed");
  });

  it("hooksStatus shows not installed", async () => {
    const { hooksStatus } = await import("../../src/commands/hooks.js");
    hooksStatus({});
    expect(spy.getLog()).toContain("not installed");
  });

  it("hooksStatus shows installed after install", async () => {
    const { hooksInstall, hooksStatus } = await import("../../src/commands/hooks.js");
    hooksInstall();
    spy = captureConsole();
    hooksStatus({});
    expect(spy.getLog()).toContain("installed");
    expect(spy.getLog()).not.toContain("not installed");
  });

  it("hooksStatus outputs JSON", async () => {
    const { hooksStatus } = await import("../../src/commands/hooks.js");
    hooksStatus({ json: true });
    const parsed = JSON.parse(spy.getLog());
    expect(parsed).toEqual({ installed: false });
  });
});
