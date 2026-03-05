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

describe("settings set", () => {
  let settingsSet: typeof import("../../src/commands/settings/set.js").settingsSet;
  let loadSettings: typeof import("../../src/lib/settings.js").loadSettings;
  let spy: ConsoleSpy;

  beforeEach(async () => {
    mockProcessExit();
    spy = captureConsole();
    ({ settingsSet } = await import("../../src/commands/settings/set.js"));
    ({ loadSettings } = await import("../../src/lib/settings.js"));
  });

  it("sets a string setting", () => {
    settingsSet("ide", "vscode");
    expect(loadSettings().ide).toBe("vscode");
    expect(spy.getLog()).toContain("vscode");
  });

  it("coerces boolean true values", () => {
    settingsSet("compactView", "true");
    expect(loadSettings().compactView).toBe(true);
  });

  it("coerces boolean '1' to true", () => {
    settingsSet("compactView", "1");
    expect(loadSettings().compactView).toBe(true);
  });

  it("coerces boolean false values", () => {
    settingsSet("compactView", "false");
    expect(loadSettings().compactView).toBe(false);
  });

  it("coerces boolean '0' to false", () => {
    settingsSet("compactView", "0");
    expect(loadSettings().compactView).toBe(false);
  });

  it("exits on invalid boolean value", () => {
    expect(() => settingsSet("compactView", "maybe")).toThrow(ProcessExitError);
    expect(spy.getError()).toContain("boolean");
  });

  it("coerces number values", () => {
    settingsSet("pollingIntervalMs", "5000");
    expect(loadSettings().pollingIntervalMs).toBe(5000);
  });

  it("exits on invalid number value", () => {
    expect(() => settingsSet("pollingIntervalMs", "abc")).toThrow(ProcessExitError);
    expect(spy.getError()).toContain("number");
  });

  it("exits for unknown key", () => {
    expect(() => settingsSet("nonexistent", "value")).toThrow(ProcessExitError);
    expect(spy.getError()).toContain("Unknown setting");
  });
});
