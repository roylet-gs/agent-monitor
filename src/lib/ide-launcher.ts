import { execSync } from "child_process";
import { log } from "./logger.js";
import { isTerminalOpenAt } from "./process.js";
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
  const app = detectTerminalApp();

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
          // Small delay to let the terminal window open before sending the command
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
    log("info", "ide", `Opened claude in ${app} at ${worktreePath} (continue=${continueSession})`);
  } catch (err) {
    log("error", "ide", `Failed to open claude in ${app}: ${err}`);
    throw new Error(`Failed to open terminal. Is ${app} available?`);
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
    }
    log("info", "ide", `Opened ${worktreePath} in ${ide}`);
  } catch (err) {
    log("error", "ide", `Failed to open ${worktreePath} in ${ide}: ${err}`);
    throw new Error(`Failed to open in ${ide}. Is it installed and in PATH?`);
  }
}
