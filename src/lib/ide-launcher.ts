import { execSync } from "child_process";
import { randomBytes } from "crypto";
import path from "path";
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

const AM_TITLE_PREFIX = "[am]";

function generateWindowId(): string {
  return randomBytes(3).toString("hex");
}

function makeWindowTitle(title: string | undefined, worktreePath: string, windowId: string): string {
  return `${AM_TITLE_PREFIX} ${title ?? path.basename(worktreePath)} #${windowId}`;
}

function tryFocusTerminalWindow(app: string, worktreePath: string, title: string | undefined): boolean {
  // Match on prefix + display name (without the #id suffix) so we find any window for this worktree
  const matchStr = `${AM_TITLE_PREFIX} ${title ?? path.basename(worktreePath)}`;
  const escapedTitle = matchStr.replace(/"/g, '\\"');
  let script: string;

  switch (app) {
    case "Terminal":
      script = `
        tell application "Terminal"
          repeat with w in windows
            if custom title of tab 1 of w contains "${escapedTitle}" then
              set index of w to 1
              activate
              return "found"
            end if
          end repeat
        end tell
        return "not_found"
      `;
      break;
    case "iTerm2":
      script = `
        tell application "iTerm2"
          repeat with w in windows
            repeat with t in tabs of w
              repeat with s in sessions of t
                if name of s contains "${escapedTitle}" then
                  select t
                  tell w to select
                  activate
                  return "found"
                end if
              end repeat
            end repeat
          end repeat
        end tell
        return "not_found"
      `;
      break;
    default:
      // Ghostty, Warp, and other apps: use System Events to find window by title
      script = `
        set matched to false
        tell application "System Events"
          if not (exists process "${app}") then return "not_found"
          tell process "${app}"
            repeat with w in windows
              if title of w contains "${escapedTitle}" then
                perform action "AXRaise" of w
                set matched to true
                exit repeat
              end if
            end repeat
          end tell
        end tell
        if matched then
          tell application "${app}" to activate
          return "found"
        end if
        return "not_found"
      `;
      break;
  }

  try {
    const result = execSync(`osascript <<'APPLESCRIPT'\n${script}\nAPPLESCRIPT`, {
      encoding: "utf-8",
      timeout: 3000,
      shell: "/bin/bash",
    }).trim();
    log("debug", "terminal", `tryFocusTerminalWindow(${app}, ${matchStr}): ${result}`);
    return result === "found";
  } catch (err) {
    log("debug", "terminal", `tryFocusTerminalWindow failed for ${app}: ${err}`);
    return false;
  }
}

function setTerminalTitle(app: string, windowTitle: string): void {
  const escapedTitle = windowTitle.replace(/"/g, '\\"');

  try {
    let script: string;
    switch (app) {
      case "Terminal":
        script = `
          tell application "Terminal"
            set custom title of tab 1 of front window to "${escapedTitle}"
          end tell
        `;
        break;
      case "iTerm2":
        script = `
          tell application "iTerm2"
            tell current session of current tab of current window
              set name to "${escapedTitle}"
            end tell
          end tell
        `;
        break;
      default:
        // For Ghostty, Warp, etc. — use escape sequence via keystroke
        execSync(
          `osascript -e 'tell application "System Events" to keystroke "printf ${`'\\''`}\\\\e]0;${escapedTitle}\\\\a${`'\\''`}" & return'`,
          { stdio: "ignore", timeout: 2000 }
        );
        return;
    }
    execSync(`osascript <<'APPLESCRIPT'\n${script}\nAPPLESCRIPT`, {
      stdio: "ignore",
      timeout: 2000,
      shell: "/bin/bash",
    });
  } catch (err) {
    log("debug", "terminal", `Failed to set terminal title: ${err}`);
  }
}

export function openTerminal(worktreePath: string, title?: string): string | undefined {
  const app = detectTerminalApp();
  const escapedPath = worktreePath.replace(/"/g, '\\"');

  if (tryFocusTerminalWindow(app, worktreePath, title)) {
    log("info", "terminal", `Re-focused ${app} window for ${worktreePath}`);
    return;
  }

  const windowId = generateWindowId();
  const windowTitle = makeWindowTitle(title, worktreePath, windowId);
  log("info", "terminal", `Opening new ${app} window at ${worktreePath} (${windowTitle})`);

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
    setTerminalTitle(app, windowTitle);
  } else {
    execSync(`x-terminal-emulator --working-directory="${worktreePath}"`, {
      stdio: "ignore",
    });
  }
  return windowId;
}

export function openClaudeInTerminal(worktreePath: string, continueSession: boolean, title?: string): string {
  const escapedPath = worktreePath.replace(/"/g, '\\"');
  const claudeCmd = continueSession ? "claude -c" : "claude";
  const app = detectTerminalApp();
  const windowId = generateWindowId();
  const windowTitle = makeWindowTitle(title, worktreePath, windowId);

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
      setTerminalTitle(app, windowTitle);
    } else {
      execSync(
        `x-terminal-emulator --working-directory="${escapedPath}" -e "${claudeCmd}"`,
        { stdio: "ignore" }
      );
    }
    log("info", "ide", `Opened claude in ${app} at ${worktreePath} (${windowTitle})`);
  } catch (err) {
    log("error", "ide", `Failed to open claude in ${app}: ${err}`);
    throw new Error(`Failed to open terminal. Is ${app} available?`);
  }
  return windowId;
}

export function openInIde(worktreePath: string, ide: Settings["ide"], title?: string): string | undefined {
  try {
    switch (ide) {
      case "cursor":
        execSync(`cursor "${worktreePath}"`, { stdio: "ignore" });
        break;
      case "vscode":
        execSync(`code "${worktreePath}"`, { stdio: "ignore" });
        break;
      case "terminal":
        return openTerminal(worktreePath, title);
    }
    log("info", "ide", `Opened ${worktreePath} in ${ide}`);
  } catch (err) {
    log("error", "ide", `Failed to open ${worktreePath} in ${ide}: ${err}`);
    throw new Error(`Failed to open in ${ide}. Is it installed and in PATH?`);
  }
}
