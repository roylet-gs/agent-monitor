import { spawnSync } from "child_process";
import { log } from "./logger.js";

/**
 * Fully resets stdin/stdout to normal terminal state after Ink unmounts.
 * Ink may leave stdin paused, in raw mode, or with lingering listeners.
 */
function resetTerminal(): void {
  // Restore cooked mode (echo + line buffering)
  if (process.stdin.isTTY) {
    try {
      process.stdin.setRawMode(false);
    } catch (err) {
      log("debug", "scripts", `Failed to reset raw mode: ${err}`);
    }
  }

  // Remove any lingering listeners Ink may have left
  process.stdin.removeAllListeners("readable");
  process.stdin.removeAllListeners("data");
  process.stdin.removeAllListeners("keypress");
  process.stdin.removeAllListeners("end");

  // Pause stdin so Node releases its read on fd 0.
  // The child process needs exclusive access via stdio: "inherit".
  if (!process.stdin.isPaused()) {
    process.stdin.pause();
  }

  // Unref so it doesn't keep the process alive on its own
  process.stdin.unref();
}

/**
 * Runs a startup script with full terminal passthrough.
 * Uses spawnSync so the child process gets exclusive terminal access
 * without any interference from Node's stream layer.
 */
export function runScript(scriptPath: string, cwd: string): number {
  resetTerminal();

  console.log(`\n\x1b[36m── Running startup script: ${scriptPath}\x1b[0m`);
  console.log(`\x1b[2m   cwd: ${cwd}\x1b[0m\n`);

  const result = spawnSync(scriptPath, [], {
    stdio: "inherit",
    shell: true,
    cwd,
    env: { ...process.env },
  });

  const exitCode = result.status ?? 1;

  if (result.error) {
    console.error(`\n\x1b[31m── Script error: ${result.error.message}\x1b[0m`);
    log("error", "scripts", `Startup script error: ${result.error.message}`);
  } else if (exitCode === 0) {
    console.log(`\n\x1b[32m── Script finished (exit code 0)\x1b[0m`);
  } else {
    console.log(`\n\x1b[31m── Script failed (exit code ${exitCode})\x1b[0m`);
  }

  log("info", "scripts", `Startup script exited with code ${exitCode}`);
  return exitCode;
}

/**
 * Wait for the user to press Enter before continuing.
 */
export function waitForEnter(message = "Press Enter to continue..."): void {
  process.stdout.write(`\x1b[2m${message}\x1b[0m`);

  // Use spawnSync to read a line — avoids Node stdin stream issues
  spawnSync("read", ["-r"], {
    stdio: "inherit",
    shell: true,
  });
}
