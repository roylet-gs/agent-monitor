import { execSync } from "child_process";
import { realpathSync } from "fs";
import { log } from "./logger.js";
import type { Settings } from "./types.js";

function detectTerminalApp(): string {
  const term = process.env.TERM_PROGRAM;
  switch (term) {
    case "Apple_Terminal":
      return "Terminal";
    case "iTerm.app":
      return "iTerm2";
    case "WarpTerminal":
      return "Warp";
    case "ghostty":
      return "Ghostty";
    default:
      return "Terminal";
  }
}

export function isTerminalOpenAt(worktreePath: string): boolean {
  try {
    const realPath = realpathSync(worktreePath);
    const output = execSync("lsof -d cwd -c zsh -c bash -c fish -c nu -Fn 2>/dev/null", {
      timeout: 2000,
      encoding: "utf-8",
    });
    for (const line of output.split("\n")) {
      if (line.startsWith("n") && line.substring(1) === realPath) {
        return true;
      }
    }
    return false;
  } catch {
    return false;
  }
}

export function openTerminal(worktreePath: string): void {
  const app = detectTerminalApp();
  const escapedPath = worktreePath.replace(/"/g, '\\"');

  if (isTerminalOpenAt(worktreePath)) {
    log("info", "terminal", `Re-focusing ${app} (already open at ${worktreePath})`);
    execSync(`osascript -e 'tell application "${app}" to activate'`, { stdio: "ignore" });
    return;
  }

  log("info", "terminal", `Opening new ${app} window at ${worktreePath}`);

  if (process.platform === "darwin") {
    switch (app) {
      case "Terminal":
        execSync(
          `osascript -e 'tell app "Terminal" to do script "cd ${escapedPath}"'`,
          { stdio: "ignore" }
        );
        break;
      case "iTerm2":
        execSync(
          `osascript -e 'tell app "iTerm2" to create window with default profile command "cd ${escapedPath} && exec $SHELL"'`,
          { stdio: "ignore" }
        );
        break;
      default:
        execSync(`open -a "${app}" "${worktreePath}"`, { stdio: "ignore" });
        break;
    }
  } else {
    execSync(`x-terminal-emulator --working-directory="${worktreePath}"`, {
      stdio: "ignore",
    });
  }
}

export function openClaudeInTerminal(
  worktreePath: string,
  options: { continueSession: boolean; prompt?: string },
): void {
  const { continueSession, prompt } = options;
  const app = detectTerminalApp();

  // Re-focus if a terminal is already open at this path
  if (isTerminalOpenAt(worktreePath)) {
    log("info", "ide", `Re-focusing ${app} (claude already open at ${worktreePath})`);
    execSync(`osascript -e 'tell application "${app}" to activate'`, { stdio: "ignore" });
    return;
  }

  const escapedPath = worktreePath.replace(/"/g, '\\"');
  let claudeCmd: string;
  if (continueSession) {
    claudeCmd = "claude -c";
  } else if (prompt) {
    // Shell-escape the prompt for embedding in AppleScript double-quoted strings
    const escapedPrompt = prompt
      .replace(/\\/g, "\\\\")
      .replace(/"/g, '\\"')
      .replace(/'/g, "'\\''")
      .replace(/\n/g, " ");
    claudeCmd = `claude --prompt '${escapedPrompt.replace(/'/g, "'\\''")}'`;
  } else {
    claudeCmd = "claude";
  }

  try {
    if (process.platform === "darwin") {
      switch (app) {
        case "Terminal":
          execSync(
            `osascript -e 'tell app "Terminal" to do script "cd \\"${escapedPath}\\" && ${claudeCmd}"'`,
            { stdio: "ignore" }
          );
          break;
        case "iTerm2":
          execSync(
            `osascript -e 'tell app "iTerm2" to create window with default profile command "cd \\"${escapedPath}\\" && ${claudeCmd}"'`,
            { stdio: "ignore" }
          );
          break;
        default:
          execSync(`open -a "${app}" "${worktreePath}"`, { stdio: "ignore" });
          execSync(
            `sleep 0.5 && osascript -e 'tell application "System Events" to keystroke "cd \\"${escapedPath}\\" && ${claudeCmd}\n"'`,
            { stdio: "ignore" }
          );
          break;
      }
    } else {
      execSync(
        `x-terminal-emulator --working-directory="${escapedPath}" -e "${claudeCmd}"`,
        { stdio: "ignore" }
      );
    }
    log("info", "ide", `Opened claude in ${app} at ${worktreePath} (continue=${continueSession}, prompt=${!!prompt})`);
  } catch (err) {
    log("error", "ide", `Failed to open claude in ${app}: ${err}`);
    throw new Error(`Failed to open terminal. Is ${app} available?`);
  }
}

export async function launchClaudeSession(
  worktreePath: string,
  options: { continueSession: boolean; prompt?: string },
): Promise<number | null> {
  // Snapshot existing claude PIDs before launch
  let pidsBefore: Set<number>;
  try {
    const out = execSync("pgrep -x claude 2>/dev/null", { encoding: "utf-8", timeout: 2000 });
    pidsBefore = new Set(out.trim().split("\n").filter(Boolean).map(Number));
  } catch {
    pidsBefore = new Set();
  }

  openClaudeInTerminal(worktreePath, options);

  // Wait for the process to start
  await new Promise((r) => setTimeout(r, 1500));

  // Snapshot again to find new PIDs
  let pidsAfter: Set<number>;
  try {
    const out = execSync("pgrep -x claude 2>/dev/null", { encoding: "utf-8", timeout: 2000 });
    pidsAfter = new Set(out.trim().split("\n").filter(Boolean).map(Number));
  } catch {
    pidsAfter = new Set();
  }

  const newPids = [...pidsAfter].filter((p) => !pidsBefore.has(p));
  if (newPids.length === 0) return null;

  // Check CWD of new PIDs to find the one matching our worktree
  const realPath = realpathSync(worktreePath);
  for (const pid of newPids) {
    try {
      const lsofOut = execSync(`lsof -p ${pid} -d cwd -Fn 2>/dev/null`, {
        encoding: "utf-8",
        timeout: 2000,
      });
      for (const line of lsofOut.split("\n")) {
        if (line.startsWith("n") && line.substring(1) === realPath) {
          log("info", "ide", `Discovered claude PID ${pid} for ${worktreePath}`);
          return pid;
        }
      }
    } catch {
      // ignore
    }
  }

  // Fallback: return the first new PID if only one
  if (newPids.length === 1) {
    log("info", "ide", `Using single new claude PID ${newPids[0]} for ${worktreePath} (CWD mismatch)`);
    return newPids[0];
  }

  return null;
}

/**
 * Kill a claude session and close its terminal window.
 * Walks up the process tree from the claude PID to find the shell,
 * then kills the shell's process group which closes the terminal tab.
 */
export function killClaudeSession(pid: number): void {
  try {
    // Walk up process tree: claude → node → shell (terminal tab)
    // Use ps to get the full ancestor chain
    const out = execSync(
      `ps -o pid=,ppid=,comm= -p ${pid} 2>/dev/null`,
      { encoding: "utf-8", timeout: 2000 }
    ).trim();

    if (!out) {
      // Process already gone
      return;
    }

    // Walk up parents to find the shell
    let currentPid = pid;
    let shellPid: number | null = null;
    for (let i = 0; i < 10; i++) {
      try {
        const info = execSync(
          `ps -o ppid=,comm= -p ${currentPid} 2>/dev/null`,
          { encoding: "utf-8", timeout: 2000 }
        ).trim();
        if (!info) break;

        const ppid = parseInt(info.split(/\s+/)[0], 10);
        const comm = info.split(/\s+/).slice(1).join(" ").replace(/^-/, "");

        if (/^(bash|zsh|fish|sh|nu)$/.test(comm)) {
          shellPid = currentPid === pid ? ppid : currentPid;
          // Keep going up one more — the shell's parent might be login/terminal
          // but we want the shell itself
          shellPid = ppid;
          break;
        }

        if (ppid <= 1) break;
        currentPid = ppid;
      } catch {
        break;
      }
    }

    if (shellPid && shellPid > 1) {
      // Kill the shell's process group — this closes the terminal tab
      try {
        process.kill(-shellPid, "SIGHUP");
        log("info", "ide", `Killed shell process group ${shellPid} for claude PID ${pid}`);
        return;
      } catch {
        // Process group kill failed, fall through
      }
    }

    // Fallback: just kill the claude process directly
    process.kill(pid, "SIGTERM");
    log("info", "ide", `Sent SIGTERM to claude PID ${pid} (no shell parent found)`);
  } catch (err) {
    log("warn", "ide", `Failed to kill claude session PID ${pid}: ${err}`);
  }
}

export function openInIde(worktreePath: string, ide: Settings["ide"]): void {
  try {
    switch (ide) {
      case "cursor":
        execSync(`cursor "${worktreePath}"`, { stdio: "ignore" });
        break;
      case "vscode":
        execSync(`code "${worktreePath}"`, { stdio: "ignore" });
        break;
      case "terminal":
        openTerminal(worktreePath);
        break;
      case "managed":
        // No-op: TUI handles this via mode transition to role-select
        break;
    }
    log("info", "ide", `Opened ${worktreePath} in ${ide}`);
  } catch (err) {
    log("error", "ide", `Failed to open ${worktreePath} in ${ide}: ${err}`);
    throw new Error(`Failed to open in ${ide}. Is it installed and in PATH?`);
  }
}
