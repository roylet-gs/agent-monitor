import { describe, it, expect, vi, beforeEach } from "vitest";
import { writeFileSync, mkdirSync } from "fs";

vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

// Mock isGhAvailable to avoid running gh in tests
vi.mock("../../src/lib/github.js", () => ({
  isGhAvailable: vi.fn(() => true),
}));

describe("settings", () => {
  let settings: typeof import("../../src/lib/settings.js");
  let paths: typeof import("../../src/lib/paths.js");

  beforeEach(async () => {
    paths = await import("../../src/lib/paths.js");
    settings = await import("../../src/lib/settings.js");
  });

  describe("isFirstRun", () => {
    it("returns true when settings file does not exist", () => {
      expect(settings.isFirstRun()).toBe(true);
    });

    it("returns true when setupCompleted is not set", () => {
      mkdirSync(paths.APP_DIR, { recursive: true });
      writeFileSync(paths.SETTINGS_PATH, JSON.stringify({ ide: "cursor" }));
      expect(settings.isFirstRun()).toBe(true);
    });

    it("returns false when setupCompleted is true", () => {
      mkdirSync(paths.APP_DIR, { recursive: true });
      writeFileSync(paths.SETTINGS_PATH, JSON.stringify({ setupCompleted: true }));
      expect(settings.isFirstRun()).toBe(false);
    });

    it("returns true for invalid JSON", () => {
      mkdirSync(paths.APP_DIR, { recursive: true });
      writeFileSync(paths.SETTINGS_PATH, "not json");
      expect(settings.isFirstRun()).toBe(true);
    });
  });

  describe("loadSettings", () => {
    it("returns defaults when no file exists", () => {
      const result = settings.loadSettings();
      expect(result.ide).toBe("cursor");
      expect(result.defaultBranchPrefix).toBe("feature/");
    });

    it("merges saved settings over defaults", () => {
      mkdirSync(paths.APP_DIR, { recursive: true });
      writeFileSync(paths.SETTINGS_PATH, JSON.stringify({ ide: "vscode", pollingIntervalMs: 5000 }));
      const result = settings.loadSettings();
      expect(result.ide).toBe("vscode");
      expect(result.pollingIntervalMs).toBe(5000);
      // Should still have defaults for missing keys
      expect(result.defaultBranchPrefix).toBe("feature/");
    });

    it("returns defaults for invalid JSON", () => {
      mkdirSync(paths.APP_DIR, { recursive: true });
      writeFileSync(paths.SETTINGS_PATH, "broken!");
      const result = settings.loadSettings();
      expect(result.ide).toBe("cursor");
    });
  });

  describe("saveSettings", () => {
    it("writes settings to file", () => {
      const toSave = { ...settings.DEFAULT_SETTINGS, ide: "vscode" as const };
      settings.saveSettings(toSave);
      const loaded = settings.loadSettings();
      expect(loaded.ide).toBe("vscode");
    });

    it("creates APP_DIR if missing", () => {
      // APP_DIR is a temp dir that might not exist yet after reset
      settings.saveSettings(settings.DEFAULT_SETTINGS);
      // If it didn't throw, it created the dir
      const loaded = settings.loadSettings();
      expect(loaded.ide).toBe("cursor");
    });
  });
});
