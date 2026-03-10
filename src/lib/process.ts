import { execSync } from "child_process";
import { execFile } from "child_process";
import { realpathSync } from "fs";
import { log } from "./logger.js";

function execFileAsync(cmd: string, args: string[], timeout: number): Promise<string> {
  return new Promise((resolve, reject) => {
    execFile(cmd, args, { timeout, encoding: "utf-8" }, (err, stdout) => {
      if (err) {
        reject(err);
      } else {
        resolve(stdout);
      }
    });
  });
}

/**
 * Runs a single `lsof` call and returns all paths where a shell process has its cwd.
 * This is batched so we only pay the lsof cost once per polling cycle regardless of worktree count.
 */
export function getTerminalPaths(): Set<string> {
  const paths = new Set<string>();
  try {
    const output = execSync("lsof -d cwd -c zsh -c bash -c fish -c nu -Fn 2>/dev/null", {
      timeout: 3000,
      encoding: "utf-8",
    });
    for (const line of output.split("\n")) {
      if (line.startsWith("n") && line.length > 1) {
        paths.add(line.substring(1));
      }
    }
    log("debug", "process", `getTerminalPaths found ${paths.size} shell cwd paths`);
  } catch {
    log("debug", "process", "lsof returned no results or failed");
  }
  return paths;
}

/**
 * Runs a single `ps` call and returns paths where Cursor or VS Code have a workspace open.
 * Parses CLI arguments from process listings to find workspace folder paths.
 */
export function getIdePaths(): Map<string, "cursor" | "vscode"> {
  const paths = new Map<string, "cursor" | "vscode">();
  try {
    const output = execSync("ps -eo args 2>/dev/null", {
      timeout: 3000,
      encoding: "utf-8",
    });
    for (const line of output.split("\n")) {
      const trimmed = line.trim();
      let ide: "cursor" | "vscode" | null = null;
      if (trimmed.includes("/Cursor.app/") || trimmed.startsWith("cursor ")) {
        ide = "cursor";
      } else if (trimmed.includes("/Code.app/") || trimmed.startsWith("code ")) {
        ide = "vscode";
      }
      if (!ide) continue;

      // Extract workspace path: last argument that looks like an absolute path
      const parts = trimmed.split(/\s+/);
      for (let i = parts.length - 1; i >= 1; i--) {
        if (parts[i].startsWith("/") && !parts[i].startsWith("/--")) {
          try {
            const resolved = realpathSync(parts[i]);
            paths.set(resolved, ide);
          } catch {
            // path doesn't exist, skip
          }
          break;
        }
      }
    }
    log("debug", "process", `getIdePaths found ${paths.size} IDE workspace paths`);
  } catch {
    log("debug", "process", "ps returned no results or failed");
  }
  return paths;
}

/**
 * Find and kill Claude Code processes whose cwd matches the given path.
 * Uses lsof to find node processes at the path, then verifies via ps that they're Claude.
 * Returns the number of processes killed.
 */
export function killClaudeAtPath(targetPath: string): number {
  let killed = 0;
  try {
    const realPath = realpathSync(targetPath);
    // Find node processes with cwd at the target path
    const output = execSync(`lsof -d cwd -c node -Fpn 2>/dev/null`, {
      timeout: 3000,
      encoding: "utf-8",
    });

    let currentPid: number | null = null;
    for (const line of output.split("\n")) {
      if (line.startsWith("p")) {
        currentPid = parseInt(line.substring(1), 10);
      } else if (line.startsWith("n") && currentPid !== null) {
        const cwdPath = line.substring(1);
        if (cwdPath === realPath) {
          // Verify this is actually a Claude process
          try {
            const args = execSync(`ps -p ${currentPid} -o args= 2>/dev/null`, {
              timeout: 2000,
              encoding: "utf-8",
            }).trim();
            if (args.includes("claude")) {
              process.kill(currentPid, "SIGTERM");
              killed++;
              log("info", "process", `Killed Claude process ${currentPid} at ${realPath}`);
            }
          } catch {
            // Process may have already exited
          }
        }
        currentPid = null;
      }
    }
  } catch {
    log("debug", "process", `killClaudeAtPath: lsof returned no results or failed for ${targetPath}`);
  }
  return killed;
}

/**
 * Check if a terminal shell has its cwd at the given path.
 * Convenience wrapper for single-path checks (used by ide-launcher).
 */
export function isTerminalOpenAt(worktreePath: string): boolean {
  try {
    const realPath = realpathSync(worktreePath);
    const paths = getTerminalPaths();
    return paths.has(realPath);
  } catch {
    return false;
  }
}

/**
 * Async version of getTerminalPaths. Uses execFile instead of execSync
 * so the event loop is not blocked during the lsof call.
 */
export async function getTerminalPathsAsync(): Promise<Set<string>> {
  const paths = new Set<string>();
  try {
    const output = await execFileAsync(
      "lsof",
      ["-d", "cwd", "-c", "zsh", "-c", "bash", "-c", "fish", "-c", "nu", "-Fn"],
      3000
    );
    for (const line of output.split("\n")) {
      if (line.startsWith("n") && line.length > 1) {
        paths.add(line.substring(1));
      }
    }
    log("debug", "process", `getTerminalPathsAsync found ${paths.size} shell cwd paths`);
  } catch {
    log("debug", "process", "lsof async returned no results or failed");
  }
  return paths;
}

/**
 * Async version of getIdePaths. Uses execFile instead of execSync
 * so the event loop is not blocked during the ps call.
 */
export async function getIdePathsAsync(): Promise<Map<string, "cursor" | "vscode">> {
  const paths = new Map<string, "cursor" | "vscode">();
  try {
    const output = await execFileAsync("ps", ["-eo", "args"], 3000);
    for (const line of output.split("\n")) {
      const trimmed = line.trim();
      let ide: "cursor" | "vscode" | null = null;
      if (trimmed.includes("/Cursor.app/") || trimmed.startsWith("cursor ")) {
        ide = "cursor";
      } else if (trimmed.includes("/Code.app/") || trimmed.startsWith("code ")) {
        ide = "vscode";
      }
      if (!ide) continue;

      const parts = trimmed.split(/\s+/);
      for (let i = parts.length - 1; i >= 1; i--) {
        if (parts[i].startsWith("/") && !parts[i].startsWith("/--")) {
          try {
            const resolved = realpathSync(parts[i]);
            paths.set(resolved, ide);
          } catch {
            // path doesn't exist, skip
          }
          break;
        }
      }
    }
    log("debug", "process", `getIdePathsAsync found ${paths.size} IDE workspace paths`);
  } catch {
    log("debug", "process", "ps async returned no results or failed");
  }
  return paths;
}
