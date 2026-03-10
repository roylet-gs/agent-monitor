import { execSync } from "child_process";
import { randomBytes } from "crypto";
import path from "path";
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

function openGhosttyTab(command: string, windowTitle: string): void {
  const escapedTitle = windowTitle.replace(/"/g, '\\"');
  const escapedCmd = command.replace(/\\/g, "\\\\").replace(/"/g, '\\"');
  const script = `
    tell application "Ghostty" to activate
    delay 0.3
    tell application "System Events"
      tell process "Ghostty"
        keystroke "t" using command down
      end tell
    end tell
    delay 0.3
    tell application "System Events"
      tell process "Ghostty"
        keystroke "${escapedCmd}" & return
        delay 0.1
        keystroke "printf '\\\\e]0;${escapedTitle}\\\\a'" & return
      end tell
    end tell
  `;
  try {
    execSync(`osascript <<'APPLESCRIPT'\n${script}\nAPPLESCRIPT`, {
      stdio: "ignore",
      timeout: 5000,
      shell: "/bin/bash",
    });
  } catch (err) {
    log("warn", "terminal", `openGhosttyTab failed: ${err}`);
  }
}

function openITermTab(command: string): void {
  const escapedCmd = command.replace(/"/g, '\\"');
  const script = `
    tell application "iTerm2"
      activate
      if (count of windows) > 0 then
        tell current window
          create tab with default profile command "${escapedCmd}"
        end tell
      else
        create window with default profile command "${escapedCmd}"
      end if
    end tell
  `;
  try {
    execSync(`osascript <<'APPLESCRIPT'\n${script}\nAPPLESCRIPT`, {
      stdio: "ignore",
      timeout: 5000,
      shell: "/bin/bash",
    });
  } catch (err) {
    log("warn", "terminal", `openITermTab failed: ${err}`);
  }
}

export function focusTerminal(worktreePath: string, title?: string): boolean {
  const app = detectTerminalApp();
  // Tier 1: try exact title match (works well for Terminal.app, iTerm2)
  if (tryFocusTerminalWindow(app, worktreePath, title)) {
    log("info", "terminal", `Re-focused ${app} window for ${worktreePath}`);
    return true;
  }
  // Tier 2: just activate the terminal app (guaranteed to bring it forward)
  try {
    execSync(`osascript -e 'tell application "${app}" to activate'`, {
      stdio: "ignore", timeout: 2000,
    });
    log("info", "terminal", `Activated ${app} (title match failed, but terminal is open)`);
    return true;
  } catch {
    return false;
  }
}

export function openTerminal(worktreePath: string, title?: string): string | undefined {
  const app = detectTerminalApp();
  const escapedPath = worktreePath.replace(/"/g, '\\"');

  if (tryFocusTerminalWindow(app, worktreePath, title)) {
    log("info", "terminal", `Re-focused ${app} window for ${worktreePath}`);
    return;
  }

  // Check if a terminal IS open but we just couldn't find the window by title
  if (isTerminalOpenAt(worktreePath)) {
    try {
      execSync(`osascript -e 'tell application "${app}" to activate'`, {
        stdio: "ignore", timeout: 2000,
      });
      log("info", "terminal", `Activated ${app} (terminal detected at ${worktreePath})`);
    } catch { /* ignore */ }
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
        openITermTab(`cd ${escapedPath} && exec $SHELL`);
        break;
      case "Ghostty":
        openGhosttyTab(`cd "${escapedPath}" && exec $SHELL`, windowTitle);
        return windowId; // Ghostty sets title via escape sequence in helper
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
          openITermTab(`cd \\"${escapedPath}\\" && ${claudeCmd}`);
          break;
        case "Ghostty":
          openGhosttyTab(`cd "${escapedPath}" && ${claudeCmd}`, windowTitle);
          log("info", "ide", `Opened claude in ${app} at ${worktreePath} (${windowTitle})`);
          return windowId;
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
