import { describe, it, expect, vi, beforeEach } from "vitest";
import { captureConsole, type ConsoleSpy } from "../helpers/console-capture.js";
import { ProcessExitError, mockProcessExit } from "../helpers/process-exit.js";

vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

vi.mock("../../src/lib/github.js", () => ({
  isGhAvailable: vi.fn(() => true),
}));

describe("settings get", () => {
  let settingsGet: typeof import("../../src/commands/settings/get.js").settingsGet;
  let spy: ConsoleSpy;

  beforeEach(async () => {
    mockProcessExit();
    spy = captureConsole();
    ({ settingsGet } = await import("../../src/commands/settings/get.js"));
  });

  it("outputs a setting value", () => {
    settingsGet("ide", {});
    expect(spy.getLog()).toBe("cursor");
  });

  it("outputs JSON when --json flag is set", () => {
    settingsGet("ide", { json: true });
    const parsed = JSON.parse(spy.getLog());
    expect(parsed).toEqual({ key: "ide", value: "cursor" });
  });

  it("exits with error for unknown key", () => {
    expect(() => settingsGet("nonexistent", {})).toThrow(ProcessExitError);
    expect(spy.getError()).toContain("Unknown setting");
  });
});
