import { execSync } from "child_process";
import { log } from "./logger.js";
import type { Settings } from "./types.js";

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
    }
    log("info", "ide", `Opened ${worktreePath} in ${ide}`);
  } catch (err) {
    log("error", "ide", `Failed to open ${worktreePath} in ${ide}: ${err}`);
    throw new Error(`Failed to open in ${ide}. Is it installed and in PATH?`);
  }
}
