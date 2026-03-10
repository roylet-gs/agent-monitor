import { describe, it, expect, vi, beforeEach } from "vitest";
import { mkdtempSync, writeFileSync, existsSync, unlinkSync } from "fs";
import { tmpdir } from "os";
import { join } from "path";

const { tempDir, testPidPath } = vi.hoisted(() => {
  const { mkdtempSync, mkdirSync } = require("fs");
  const { tmpdir } = require("os");
  const { join } = require("path");
  const tempDir = mkdtempSync(join(tmpdir(), "am-daemon-test-"));
  mkdirSync(tempDir, { recursive: true });
  return { tempDir, testPidPath: join(tempDir, "am.daemon.pid") };
});

vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

vi.mock("../../src/lib/paths.js", () => ({
  APP_DIR: tempDir,
  DB_PATH: join(tempDir, "agent-monitor.db"),
  SETTINGS_PATH: join(tempDir, "settings.json"),
  LOG_PATH: join(tempDir, "debug.log"),
  SOCKET_PATH: join(tempDir, "am.sock"),
  RULES_PATH: join(tempDir, "rules.json"),
  AM_MANAGED_PERMISSIONS_PATH: join(tempDir, "am-managed-permissions.json"),
  DAEMON_PID_PATH: testPidPath,
}));

import { getDaemonPid, isDaemonRunning, stopDaemon } from "../../src/lib/daemon.js";

describe("daemon PID utilities", () => {
  beforeEach(() => {
    try { unlinkSync(testPidPath); } catch { /* ignore */ }
  });

  it("returns null when no PID file exists", () => {
    expect(getDaemonPid()).toBeNull();
  });

  it("returns null for non-running PID", () => {
    writeFileSync(testPidPath, "99999999");
    expect(getDaemonPid()).toBeNull();
    // Should also clean up the stale PID file
    expect(existsSync(testPidPath)).toBe(false);
  });

  it("returns PID for current process", () => {
    writeFileSync(testPidPath, String(process.pid));
    expect(getDaemonPid()).toBe(process.pid);
  });

  it("isDaemonRunning returns false when no PID file", () => {
    expect(isDaemonRunning()).toBe(false);
  });

  it("isDaemonRunning returns true when PID file has current process", () => {
    writeFileSync(testPidPath, String(process.pid));
    expect(isDaemonRunning()).toBe(true);
  });

  it("returns null for invalid PID content", () => {
    writeFileSync(testPidPath, "not-a-number");
    expect(getDaemonPid()).toBeNull();
  });

  it("stopDaemon returns false when no daemon running", () => {
    expect(stopDaemon()).toBe(false);
  });
});
