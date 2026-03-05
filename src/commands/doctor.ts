import { existsSync } from "fs";
import { isGlobalHooksInstalled } from "../lib/hooks-installer.js";
import { isGhAvailable } from "../lib/github.js";
import { getDb, getRepositories, getAllWorktrees } from "../lib/db.js";
import { DB_PATH, SETTINGS_PATH, APP_DIR } from "../lib/paths.js";
import { loadSettings } from "../lib/settings.js";
import { outputJson } from "../lib/output.js";

interface Check {
  name: string;
  status: "ok" | "warn" | "fail";
  message: string;
}

export function doctor(opts: { json?: boolean }): void {
  const checks: Check[] = [];

  // App directory
  checks.push({
    name: "App directory",
    status: existsSync(APP_DIR) ? "ok" : "warn",
    message: existsSync(APP_DIR) ? APP_DIR : `Missing: ${APP_DIR} (will be created on first use)`,
  });

  // Database
  try {
    const db = getDb();
    const repos = getRepositories();
    const worktrees = getAllWorktrees();
    checks.push({
      name: "Database",
      status: "ok",
      message: `${DB_PATH} — ${repos.length} repos, ${worktrees.length} worktrees`,
    });
  } catch (err) {
    checks.push({
      name: "Database",
      status: "fail",
      message: `Failed to open: ${err}`,
    });
  }

  // Settings
  checks.push({
    name: "Settings",
    status: existsSync(SETTINGS_PATH) ? "ok" : "warn",
    message: existsSync(SETTINGS_PATH) ? SETTINGS_PATH : "Using defaults (no settings.json)",
  });

  // Claude hooks
  const hooksInstalled = isGlobalHooksInstalled();
  checks.push({
    name: "Claude hooks",
    status: hooksInstalled ? "ok" : "warn",
    message: hooksInstalled ? "Installed in ~/.claude/settings.json" : "Not installed. Run: am hooks install",
  });

  // gh CLI
  const ghOk = isGhAvailable();
  const settings = loadSettings();
  checks.push({
    name: "gh CLI",
    status: ghOk ? "ok" : settings.ghPrStatus ? "warn" : "ok",
    message: ghOk ? "Available" : "Not found (PR status disabled)",
  });

  // Linear
  if (settings.linearEnabled) {
    checks.push({
      name: "Linear",
      status: settings.linearApiKey ? "ok" : "warn",
      message: settings.linearApiKey ? "Enabled with API key" : "Enabled but no API key set",
    });
  }

  if (opts.json) {
    outputJson(checks);
    return;
  }

  for (const check of checks) {
    const icon = check.status === "ok" ? "OK" : check.status === "warn" ? "WARN" : "FAIL";
    const prefix = `[${icon}]`.padEnd(7);
    console.log(`${prefix} ${check.name}: ${check.message}`);
  }
}
