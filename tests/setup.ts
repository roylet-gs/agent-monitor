import { mkdtempSync, rmSync } from "fs";
import { tmpdir } from "os";
import { join } from "path";
import { beforeEach, afterEach, vi } from "vitest";

let testDir: string;

beforeEach(() => {
  testDir = mkdtempSync(join(tmpdir(), "am-test-"));

  // Mock paths.ts to redirect all file I/O to the temp directory
  vi.doMock("../src/lib/paths.js", () => ({
    APP_DIR: testDir,
    DB_PATH: join(testDir, "agent-monitor.db"),
    SETTINGS_PATH: join(testDir, "settings.json"),
    LOG_PATH: join(testDir, "debug.log"),
    SOCKET_PATH: join(testDir, "am.sock"),
    RULES_PATH: join(testDir, "rules.json"),
    AM_MANAGED_PERMISSIONS_PATH: join(testDir, "am-managed-permissions.json"),
  }));
});

afterEach(async () => {
  // Reset all mocks and module cache
  vi.restoreAllMocks();
  vi.resetModules();

  // Close DB if open (import fresh each time since modules are reset)
  try {
    const { closeDb } = await import("../src/lib/db.js");
    closeDb();
  } catch {
    // DB module may not have been loaded
  }

  // Clean up temp directory
  try {
    rmSync(testDir, { recursive: true, force: true });
  } catch {
    // Ignore cleanup errors
  }
});

export function getTestDir(): string {
  return testDir;
}
