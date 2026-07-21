import { execSync } from "child_process";
import { execFile } from "child_process";
import { realpathSync } from "fs";
import { log } from "./logger.js";
import type { RunningProcess } from "./types.js";

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
 * Like execFileAsync but resolves with whatever was written to stdout even when
 * the command exits non-zero. `lsof` scanning every process routinely exits 1
 * (processes it can't stat) while still printing valid output — we want that
 * output, not a rejection. Rejects only when nothing was captured.
 */
function execFileAsyncLenient(cmd: string, args: string[], timeout: number): Promise<string> {
  return new Promise((resolve, reject) => {
    execFile(cmd, args, { timeout, encoding: "utf-8", maxBuffer: 16 * 1024 * 1024 }, (err, stdout) => {
      if (stdout && stdout.length > 0) {
        resolve(stdout);
      } else if (err) {
        reject(err);
      } else {
        resolve(stdout ?? "");
      }
    });
  });
}

/** Sync exec that returns stdout even on a non-zero exit (see execFileAsyncLenient). */
function execSyncLenient(cmd: string): string {
  try {
    return execSync(cmd, { timeout: 3000, encoding: "utf-8", maxBuffer: 16 * 1024 * 1024 });
  } catch (err) {
    const stdout = (err as { stdout?: string | Buffer }).stdout;
    if (stdout != null) return stdout.toString();
    throw err;
  }
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

// ---------------------------------------------------------------------------
// Running sub-process detection (dev servers, etc.)
//
// Mirrors the terminal/IDE detection above: one lsof + one ps call per poll
// cycle map every process' cwd to a friendly command label. A worktree is
// considered to have a running sub-process when any process (other than a
// shell, the Claude agent, an editor, or agent-monitor itself) has its cwd at
// the worktree root. Matching is against the worktree root only — subdirectory
// dev servers in a monorepo are not attributed to the worktree (v1 limitation).
// ---------------------------------------------------------------------------

const SHELL_COMMANDS = new Set([
  "zsh", "-zsh", "bash", "-bash", "fish", "-fish", "nu", "-nu",
  "sh", "-sh", "dash", "-dash", "login", "-login", "tmux",
]);

/**
 * Decide whether a detected process should be surfaced as a worktree
 * sub-process. Excludes the user's shell, the Claude agent (shown separately),
 * editors, and agent-monitor itself — everything else counts.
 */
function isExcludedProcess(comm: string, args: string): boolean {
  const base = comm.replace(/^-/, "");
  if (SHELL_COMMANDS.has(comm) || SHELL_COMMANDS.has(base)) return true;

  const text = args || comm;
  // Claude Code agent — reported via the Claude status, not here.
  if (/\bclaude\b/i.test(text)) return true;
  // Editors (workspace helper processes can root at the worktree).
  if (text.includes("/Cursor.app/") || text.includes("/Code.app/") || /(^|\s)(code|cursor)\b/.test(text)) return true;
  // agent-monitor itself (TUI + background daemon). A user project whose path
  // literally contains "agent-monitor" would also be excluded — acceptable for
  // this niche case.
  if (text.includes("agent-monitor") || /\/(cli|daemon)\.(t|j)sx?\b/.test(text)) return true;
  return false;
}

/**
 * Turn a full command line into a short, readable label.
 *   "node /repo/node_modules/.bin/vite"            -> "vite"
 *   "/opt/homebrew/bin/node /repo/.../pnpm dev"    -> "pnpm dev"
 *   "npm run dev"                                  -> "npm run dev"
 */
export function friendlyCommandLabel(args: string, fallbackComm: string): string {
  const trimmed = args.trim();
  if (!trimmed) return fallbackComm;
  const parts = trimmed.split(/\s+/);

  // Drop a leading node-family interpreter so we surface the actual tool being
  // run (e.g. "node .../vite" -> "vite"). Only when the next token is a script
  // path, not a flag — otherwise the interpreter is itself the command
  // (e.g. "python3 -m http.server" stays as-is).
  const interpreter = /^(.*\/)?(node|nodejs|deno|bun|tsx|ts-node)$/;
  const start = interpreter.test(parts[0]) && parts.length > 1 && !parts[1].startsWith("-") ? 1 : 0;

  const rest = parts.slice(start);
  const first = rest[0].split("/").pop() || rest[0];
  const label = [first, ...rest.slice(1)].join(" ");
  return label.length > 40 ? label.slice(0, 39) + "…" : label;
}

/** Parse `ps -eo pid=,args=` output into a pid → full command line map. */
export function parsePsArgs(psOutput: string): Map<number, string> {
  const map = new Map<number, string>();
  for (const line of psOutput.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    const sp = trimmed.indexOf(" ");
    if (sp === -1) {
      const pid = parseInt(trimmed, 10);
      if (!Number.isNaN(pid)) map.set(pid, "");
      continue;
    }
    const pid = parseInt(trimmed.slice(0, sp), 10);
    if (Number.isNaN(pid)) continue;
    map.set(pid, trimmed.slice(sp + 1).trim());
  }
  return map;
}

/**
 * Pure parser (unit-testable): given `lsof -d cwd -Fpcn` field output and a
 * pid → command-line map, returns a map of cwd path → running processes,
 * excluding shells/claude/editors/agent-monitor and any `excludePids`.
 *
 * When `filter` is non-empty, only processes whose full command line contains
 * it (case-insensitive substring) are kept — the user's "track this process"
 * criteria. Matching is against the full command line (with the friendly label
 * as fallback), so criteria like "dev" or a port number work.
 */
export function parseWorktreeProcesses(
  lsofOutput: string,
  psArgs: Map<number, string>,
  excludePids: Set<number> = new Set(),
  filter = ""
): Map<string, RunningProcess[]> {
  const needle = filter.trim().toLowerCase();
  const result = new Map<string, RunningProcess[]>();
  let pid: number | null = null;
  let comm = "";
  for (const line of lsofOutput.split("\n")) {
    if (!line) continue;
    const tag = line[0];
    const val = line.slice(1);
    if (tag === "p") {
      pid = parseInt(val, 10);
      comm = "";
    } else if (tag === "c") {
      comm = val;
    } else if (tag === "n" && pid !== null) {
      const path = val;
      const args = psArgs.get(pid) ?? "";
      if (!excludePids.has(pid) && !isExcludedProcess(comm, args)) {
        if (!needle || (args || comm).toLowerCase().includes(needle)) {
          const list = result.get(path) ?? [];
          list.push({ pid, command: friendlyCommandLabel(args, comm) });
          result.set(path, list);
        }
      }
    }
  }
  return result;
}

/**
 * The most-specific (longest) worktree root that contains `cwdPath`, or null.
 * Used so a process running inside a nested worktree is attributed to that
 * worktree and not also to an ancestor checkout (e.g. worktrees kept under
 * `<main>/.claude/worktrees/*`).
 */
function mostSpecificRoot(cwdPath: string, roots: string[]): string | null {
  let best: string | null = null;
  for (const root of roots) {
    if (cwdPath === root || cwdPath.startsWith(root + "/")) {
      if (best === null || root.length > best.length) best = root;
    }
  }
  return best;
}

/**
 * Collect the running processes attributable to `worktreeRealPath`: any process
 * whose cwd is the worktree root or a subdirectory of it (so monorepo dev
 * servers started in an app subfolder are found), excluding those that belong
 * to a more-specific worktree nested underneath. `allWorktreeRealPaths` is the
 * full set of monitored worktree roots (real paths).
 */
export function processesForWorktree(
  procMap: Map<string, RunningProcess[]>,
  worktreeRealPath: string,
  allWorktreeRealPaths: string[]
): RunningProcess[] {
  const out: RunningProcess[] = [];
  for (const [cwdPath, procs] of procMap) {
    if (cwdPath !== worktreeRealPath && !cwdPath.startsWith(worktreeRealPath + "/")) continue;
    if (mostSpecificRoot(cwdPath, allWorktreeRealPaths) === worktreeRealPath) {
      out.push(...procs);
    }
  }
  return out;
}

/** PIDs to always exclude (the scanning process and its parent). */
function selfPids(): Set<number> {
  const pids = new Set<number>([process.pid]);
  if (typeof process.ppid === "number") pids.add(process.ppid);
  return pids;
}

/**
 * Runs one `lsof` + one `ps` call and returns a map of cwd path → running
 * sub-processes for every process rooted at that path. Batched so the cost is
 * paid once per polling cycle regardless of worktree count.
 */
export function getWorktreeProcesses(filter = ""): Map<string, RunningProcess[]> {
  try {
    const lsofOutput = execSyncLenient("lsof +c 0 -d cwd -Fpcn 2>/dev/null");
    const psOutput = execSyncLenient("ps -eo pid=,args= 2>/dev/null");
    const map = parseWorktreeProcesses(lsofOutput, parsePsArgs(psOutput), selfPids(), filter);
    log("debug", "process", `getWorktreeProcesses found processes at ${map.size} paths`);
    return map;
  } catch {
    log("debug", "process", "getWorktreeProcesses lsof/ps returned no results or failed");
    return new Map();
  }
}

/** Async variant of getWorktreeProcesses (used by the daemon). */
export async function getWorktreeProcessesAsync(filter = ""): Promise<Map<string, RunningProcess[]>> {
  try {
    const [lsofOutput, psOutput] = await Promise.all([
      execFileAsyncLenient("lsof", ["+c", "0", "-d", "cwd", "-Fpcn"], 3000),
      execFileAsyncLenient("ps", ["-eo", "pid=,args="], 3000),
    ]);
    const map = parseWorktreeProcesses(lsofOutput, parsePsArgs(psOutput), selfPids(), filter);
    log("debug", "process", `getWorktreeProcessesAsync found processes at ${map.size} paths`);
    return map;
  } catch {
    log("debug", "process", "getWorktreeProcessesAsync lsof/ps returned no results or failed");
    return new Map();
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
