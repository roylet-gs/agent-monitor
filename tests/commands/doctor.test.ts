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

vi.mock("../../src/lib/github.js", () => ({
  isGhAvailable: vi.fn(() => true),
}));

describe("doctor", () => {
  let tempHome: string;
  let originalHome: string;
  let spy: ConsoleSpy;

  beforeEach(() => {
    tempHome = mkdtempSync(join(tmpdir(), "am-doctor-"));
    originalHome = process.env.HOME!;
    process.env.HOME = tempHome;
    spy = captureConsole();
  });

  afterEach(() => {
    process.env.HOME = originalHome;
    rmSync(tempHome, { recursive: true, force: true });
  });

  it("runs health checks", async () => {
    const { doctor } = await import("../../src/commands/doctor.js");
    doctor({});
    const output = spy.getLog();
    expect(output).toContain("App directory");
    expect(output).toContain("Database");
    expect(output).toContain("Settings");
    expect(output).toContain("Claude hooks");
    expect(output).toContain("gh CLI");
  });

  it("outputs JSON", async () => {
    const { doctor } = await import("../../src/commands/doctor.js");
    doctor({ json: true });
    const parsed = JSON.parse(spy.getLog());
    expect(Array.isArray(parsed)).toBe(true);
    expect(parsed.some((c: any) => c.name === "Database")).toBe(true);
  });

  it("reports database status with counts", async () => {
    const db = await import("../../src/lib/db.js");
    db.addRepository("/tmp/test", "test");
    const { doctor } = await import("../../src/commands/doctor.js");
    doctor({});
    expect(spy.getLog()).toContain("1 repos");
  });
});
