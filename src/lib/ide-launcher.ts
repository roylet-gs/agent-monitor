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

function isTerminalOpenAt(worktreePath: string): boolean {
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

export function openClaudeInTerminal(worktreePath: string, continueSession: boolean): void {
  const escapedPath = worktreePath.replace(/"/g, '\\"');
  const claudeCmd = continueSession ? "claude -c" : "claude";

  try {
    if (process.platform === "darwin") {
      execSync(
        `osascript -e 'tell app "Terminal" to do script "cd \\"${escapedPath}\\" && ${claudeCmd}"'`,
        { stdio: "ignore" }
      );
    } else {
      execSync(
        `x-terminal-emulator --working-directory="${escapedPath}" -e "${claudeCmd}"`,
        { stdio: "ignore" }
      );
    }
    log("info", "ide", `Opened claude in terminal at ${worktreePath} (continue=${continueSession})`);
  } catch (err) {
    log("error", "ide", `Failed to open claude in terminal: ${err}`);
    throw new Error("Failed to open terminal. Is Terminal.app available?");
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
        // Open a new terminal tab/window in the worktree directory
        if (process.platform === "darwin") {
          execSync(
            `osascript -e 'tell app "Terminal" to do script "cd ${worktreePath.replace(/"/g, '\\"')}"'`,
            { stdio: "ignore" }
          );
        } else {
          execSync(`x-terminal-emulator --working-directory="${worktreePath}"`, {
            stdio: "ignore",
          });
        }
        break;
      case "internal":
        // No-op: TUI handles this via mode transition to terminal-view
        break;
    }
    log("info", "ide", `Opened ${worktreePath} in ${ide}`);
  } catch (err) {
    log("error", "ide", `Failed to open ${worktreePath} in ${ide}: ${err}`);
    throw new Error(`Failed to open in ${ide}. Is it installed and in PATH?`);
  }
}
