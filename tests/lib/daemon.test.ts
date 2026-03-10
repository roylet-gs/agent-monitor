import { describe, it, expect, vi, beforeEach } from "vitest";
import { mkdtempSync, writeFileSync, existsSync, unlinkSync, mkdirSync } from "fs";
import { tmpdir } from "os";
import { join } from "path";

// Set up isolated data dir before any imports
const tempDir = mkdtempSync(join(tmpdir(), "am-daemon-test-"));
mkdirSync(tempDir, { recursive: true });
process.env.AM_DATA_DIR = tempDir;

vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

// Must not mock paths.js — daemon.ts uses DAEMON_PID_PATH directly

import { getDaemonPid, isDaemonRunning, stopDaemon } from "../../src/lib/daemon.js";
import { DAEMON_PID_PATH } from "../../src/lib/paths.js";

describe("daemon PID utilities", () => {
  beforeEach(() => {
    try { unlinkSync(DAEMON_PID_PATH); } catch { /* ignore */ }
  });

  it("returns null when no PID file exists", () => {
    expect(getDaemonPid()).toBeNull();
  });

  it("returns null for non-running PID", () => {
    writeFileSync(DAEMON_PID_PATH, "99999999");
    expect(getDaemonPid()).toBeNull();
    // Should also clean up the stale PID file
    expect(existsSync(DAEMON_PID_PATH)).toBe(false);
  });

  it("returns PID for current process", () => {
    writeFileSync(DAEMON_PID_PATH, String(process.pid));
    expect(getDaemonPid()).toBe(process.pid);
  });

  it("isDaemonRunning returns false when no PID file", () => {
    expect(isDaemonRunning()).toBe(false);
  });

  it("isDaemonRunning returns true when PID file has current process", () => {
    writeFileSync(DAEMON_PID_PATH, String(process.pid));
    expect(isDaemonRunning()).toBe(true);
  });

  it("returns null for invalid PID content", () => {
    writeFileSync(DAEMON_PID_PATH, "not-a-number");
    expect(getDaemonPid()).toBeNull();
  });

  it("stopDaemon returns false when no daemon running", () => {
    expect(stopDaemon()).toBe(false);
  });
});
