import { describe, it, expect, vi, beforeEach } from "vitest";
import { captureConsole } from "../helpers/console-capture.js";

vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

vi.mock("../../src/lib/github.js", () => ({
  isGhAvailable: vi.fn(() => true),
}));

describe("settings reset", () => {
  beforeEach(() => {
    captureConsole();
  });

  it("resets settings to defaults", async () => {
    const { settingsSet } = await import("../../src/commands/settings/set.js");
    const { settingsReset } = await import("../../src/commands/settings/reset.js");
    const { loadSettings, DEFAULT_SETTINGS } = await import("../../src/lib/settings.js");

    // Change a setting first
    settingsSet("ide", "vscode");
    expect(loadSettings().ide).toBe("vscode");

    // Reset
    settingsReset();
    const result = loadSettings();
    expect(result.ide).toBe(DEFAULT_SETTINGS.ide);
    expect(result.pollingIntervalMs).toBe(DEFAULT_SETTINGS.pollingIntervalMs);
  });
});
