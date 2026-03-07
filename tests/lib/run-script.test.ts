import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { mkdtempSync, rmSync, writeFileSync, chmodSync } from "fs";
import { join } from "path";
import { tmpdir } from "os";

vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

describe("run-script", () => {
  let tempDir: string;

  beforeEach(() => {
    tempDir = mkdtempSync(join(tmpdir(), "am-run-script-test-"));
  });

  afterEach(() => {
    rmSync(tempDir, { recursive: true, force: true });
  });

  describe("runScript", () => {
    it("pauses stdin before spawning child process", async () => {
      // Put stdin in flowing mode to simulate Ink's state
      process.stdin.resume();
      expect(process.stdin.isPaused()).toBe(false);

      const scriptPath = join(tempDir, "test.sh");
      writeFileSync(scriptPath, "#!/bin/sh\nexit 0\n");
      chmodSync(scriptPath, 0o755);

      const { runScript } = await import("../../src/lib/run-script.js");
      runScript(scriptPath, tempDir);

      // After runScript, stdin should be paused (Node released fd 0)
      expect(process.stdin.isPaused()).toBe(true);
    });

    it("returns exit code from script", async () => {
      const scriptPath = join(tempDir, "exit42.sh");
      writeFileSync(scriptPath, "#!/bin/sh\nexit 42\n");
      chmodSync(scriptPath, 0o755);

      const { runScript } = await import("../../src/lib/run-script.js");
      const exitCode = runScript(scriptPath, tempDir);

      expect(exitCode).toBe(42);
    });

    it("returns exit code 0 for successful script", async () => {
      const scriptPath = join(tempDir, "ok.sh");
      writeFileSync(scriptPath, "#!/bin/sh\necho hello\n");
      chmodSync(scriptPath, 0o755);

      const { runScript } = await import("../../src/lib/run-script.js");
      const exitCode = runScript(scriptPath, tempDir);

      expect(exitCode).toBe(0);
    });

    it("handles stdin already paused", async () => {
      // Ensure stdin is paused before calling
      process.stdin.pause();
      expect(process.stdin.isPaused()).toBe(true);

      const scriptPath = join(tempDir, "noop.sh");
      writeFileSync(scriptPath, "#!/bin/sh\nexit 0\n");
      chmodSync(scriptPath, 0o755);

      const { runScript } = await import("../../src/lib/run-script.js");
      // Should not throw
      const exitCode = runScript(scriptPath, tempDir);
      expect(exitCode).toBe(0);
      expect(process.stdin.isPaused()).toBe(true);
    });
  });
});
