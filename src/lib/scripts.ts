import { existsSync, mkdirSync, unlinkSync, writeFileSync, chmodSync } from "fs";
import { join } from "path";
import { execSync } from "child_process";
import { APP_DIR } from "./paths.js";
import { log } from "./logger.js";
import type { Settings } from "./types.js";

const SCRIPTS_DIR = join(APP_DIR, "scripts");

function ensureScriptsDir(): void {
  if (!existsSync(SCRIPTS_DIR)) {
    mkdirSync(SCRIPTS_DIR, { recursive: true });
  }
}

export function getScriptPath(repoId: string): string {
  return join(SCRIPTS_DIR, `${repoId}.sh`);
}

export function hasStartupScript(repoId: string): boolean {
  return existsSync(getScriptPath(repoId));
}

export function createStartupScript(repoId: string): string {
  ensureScriptsDir();
  const scriptPath = getScriptPath(repoId);
  if (!existsSync(scriptPath)) {
    const template = `#!/bin/bash
# Startup script for worktree creation
# This runs automatically after a new worktree is created.
# The working directory is set to the new worktree path.

# Example: install dependencies
# npm install

# Example: set up environment
# cp .env.example .env

echo "Startup script finished."
`;
    writeFileSync(scriptPath, template, "utf-8");
    chmodSync(scriptPath, 0o755);
    log("info", "scripts", `Created startup script: ${scriptPath}`);
  }
  return scriptPath;
}

export function removeStartupScript(repoId: string): void {
  const scriptPath = getScriptPath(repoId);
  if (existsSync(scriptPath)) {
    unlinkSync(scriptPath);
    log("info", "scripts", `Removed startup script: ${scriptPath}`);
  }
}

export function openScriptInEditor(repoId: string, ide: Settings["ide"]): void {
  const scriptPath = createStartupScript(repoId);
  try {
    switch (ide) {
      case "cursor":
        execSync(`cursor "${scriptPath}"`, { stdio: "ignore" });
        break;
      case "vscode":
        execSync(`code "${scriptPath}"`, { stdio: "ignore" });
        break;
      case "terminal":
        if (process.platform === "darwin") {
          execSync(
            `open -a Terminal "${scriptPath}"`,
            { stdio: "ignore" }
          );
        } else {
          execSync(`xdg-open "${scriptPath}"`, { stdio: "ignore" });
        }
        break;
    }
    log("info", "scripts", `Opened script in ${ide}: ${scriptPath}`);
  } catch (err) {
    log("error", "scripts", `Failed to open script in ${ide}: ${err}`);
    throw new Error(`Failed to open script in ${ide}. Is it installed and in PATH?`);
  }
}
