import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import type { Settings } from "../../src/lib/types.js";

const mockLog = vi.fn();
vi.mock("../../src/lib/logger.js", () => ({
  log: (...args: unknown[]) => mockLog(...args),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

const mockExecFile = vi.fn();
vi.mock("child_process", () => ({
  execFile: (...args: unknown[]) => mockExecFile(...args),
}));

describe("version", () => {
  let version: typeof import("../../src/lib/version.js");

  beforeEach(async () => {
    vi.resetModules();
    mockLog.mockClear();
    mockExecFile.mockClear();
    version = await import("../../src/lib/version.js");
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  describe("checkForUpdate", () => {
    const makeSettings = (
      overrides: Partial<Pick<Settings, "lastUpdateCheck" | "latestKnownVersion">> = {}
    ): Pick<Settings, "lastUpdateCheck" | "latestKnownVersion"> => ({
      lastUpdateCheck: undefined,
      latestKnownVersion: undefined,
      ...overrides,
    });

    it("returns cached result when checked recently", async () => {
      const settings = makeSettings({
        lastUpdateCheck: Date.now() - 1000, // 1 second ago
        latestKnownVersion: "9.9.9",
      });

      const result = await version.checkForUpdate(settings);

      expect(result).not.toBeNull();
      expect(result!.latest).toBe("9.9.9");
      expect(result!.updateAvailable).toBe(true);
      expect(mockExecFile).not.toHaveBeenCalled();
      expect(mockLog).toHaveBeenCalledWith(
        "debug",
        "version",
        expect.stringContaining("returning cached version check")
      );
    });

    it("bypasses cache when forceCheck is true", async () => {
      const settings = makeSettings({
        lastUpdateCheck: Date.now() - 1000,
        latestKnownVersion: "9.9.9",
      });

      mockExecFile.mockImplementation(
        (_cmd: string, _args: string[], _opts: unknown, cb: Function) => {
          cb(null, "2.0.0\n");
        }
      );

      const result = await version.checkForUpdate(settings, { forceCheck: true });

      expect(result).not.toBeNull();
      expect(result!.latest).toBe("2.0.0");
      expect(mockExecFile).toHaveBeenCalled();
    });

    it("passes --prefer-online flag to npm view", async () => {
      const settings = makeSettings();

      mockExecFile.mockImplementation(
        (_cmd: string, _args: string[], _opts: unknown, cb: Function) => {
          cb(null, "1.0.0\n");
        }
      );

      await version.checkForUpdate(settings);

      const args = mockExecFile.mock.calls[0][1] as string[];
      expect(args).toContain("--prefer-online");
    });

    it("logs info on successful registry fetch", async () => {
      const settings = makeSettings();

      mockExecFile.mockImplementation(
        (_cmd: string, _args: string[], _opts: unknown, cb: Function) => {
          cb(null, "2.0.0\n");
        }
      );

      await version.checkForUpdate(settings);

      expect(mockLog).toHaveBeenCalledWith(
        "info",
        "version",
        expect.stringContaining("registry version check complete")
      );
    });

    it("logs warn and returns null on failure", async () => {
      const settings = makeSettings();

      mockExecFile.mockImplementation(
        (_cmd: string, _args: string[], _opts: unknown, cb: Function) => {
          cb(new Error("network error"));
        }
      );

      const result = await version.checkForUpdate(settings);

      expect(result).toBeNull();
      expect(mockLog).toHaveBeenCalledWith(
        "warn",
        "version",
        expect.stringContaining("update check failed")
      );
    });

    it("does not call console.error on failure", async () => {
      const consoleSpy = vi.spyOn(console, "error");
      const settings = makeSettings();

      mockExecFile.mockImplementation(
        (_cmd: string, _args: string[], _opts: unknown, cb: Function) => {
          cb(new Error("network error"));
        }
      );

      await version.checkForUpdate(settings);

      expect(consoleSpy).not.toHaveBeenCalled();
      consoleSpy.mockRestore();
    });
  });
});
