import { describe, it, expect, vi, beforeEach } from "vitest";
import { captureConsole, type ConsoleSpy } from "../helpers/console-capture.js";

vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

vi.mock("../../src/lib/github.js", () => ({
  isGhAvailable: vi.fn(() => true),
}));

describe("settings list", () => {
  let settingsList: typeof import("../../src/commands/settings/list.js").settingsList;
  let spy: ConsoleSpy;

  beforeEach(async () => {
    spy = captureConsole();
    ({ settingsList } = await import("../../src/commands/settings/list.js"));
  });

  it("outputs all settings as key-value pairs", () => {
    settingsList({});
    const output = spy.getLog();
    expect(output).toContain("ide");
    expect(output).toContain("cursor");
    expect(output).toContain("pollingIntervalMs");
  });

  it("outputs JSON when --json flag is set", () => {
    settingsList({ json: true });
    const parsed = JSON.parse(spy.getLog());
    expect(parsed.ide).toBe("cursor");
    expect(parsed.defaultBranchPrefix).toBe("feature/");
  });
});
