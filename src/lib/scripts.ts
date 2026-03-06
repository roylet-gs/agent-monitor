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

export function openFileInEditor(filePath: string, ide: Settings["ide"]): void {
  try {
    switch (ide) {
      case "cursor":
        execSync(`cursor "${filePath}"`, { stdio: "ignore" });
        break;
      case "vscode":
        execSync(`code "${filePath}"`, { stdio: "ignore" });
        break;
      case "terminal":
        if (process.platform === "darwin") {
          execSync(`open -a Terminal "${filePath}"`, { stdio: "ignore" });
        } else {
          execSync(`xdg-open "${filePath}"`, { stdio: "ignore" });
        }
        break;
    }
    log("info", "scripts", `Opened file in ${ide}: ${filePath}`);
  } catch (err) {
    log("error", "scripts", `Failed to open file in ${ide}: ${err}`);
    throw new Error(`Failed to open file in ${ide}. Is it installed and in PATH?`);
  }
}

export function openScriptInEditor(repoId: string, ide: Settings["ide"]): void {
  const scriptPath = createStartupScript(repoId);
  openFileInEditor(scriptPath, ide);
}

export function openDiffInEditor(leftPath: string, rightPath: string, ide: Settings["ide"]): void {
  try {
    switch (ide) {
      case "cursor":
        execSync(`cursor --diff "${leftPath}" "${rightPath}"`, { stdio: "ignore" });
        break;
      case "vscode":
        execSync(`code --diff "${leftPath}" "${rightPath}"`, { stdio: "ignore" });
        break;
      case "terminal":
        execSync(`diff "${leftPath}" "${rightPath}" | less`, { stdio: "ignore" });
        break;
    }
    log("info", "scripts", `Opened diff in ${ide}: ${leftPath} vs ${rightPath}`);
  } catch (err) {
    log("error", "scripts", `Failed to open diff in ${ide}: ${err}`);
    throw new Error(`Failed to open diff in ${ide}. Is it installed and in PATH?`);
  }
}
