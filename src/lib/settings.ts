import { existsSync, mkdirSync, readFileSync, writeFileSync } from "fs";
import { dirname } from "path";
import { SETTINGS_PATH, APP_DIR } from "./paths.js";
import { isGhAvailable } from "./github.js";
import { log } from "./logger.js";
import type { Settings } from "./types.js";

export const DEFAULT_SETTINGS: Settings = {
  ide: "cursor",
  defaultBranchPrefix: "feature/",
  pollingIntervalMs: 2000,
  autoInstallHooks: true,
  autoSyncOnStartup: true,
  compactView: false,
  hideMainBranch: true,
  ghPrStatus: true,
  ghPollingIntervalMs: 60000,
  logLevel: "info",
  linearEnabled: false,
  linearApiKey: "",
  linearPollingIntervalMs: 60000,
};

export function loadSettings(): Settings {
  if (!existsSync(SETTINGS_PATH)) {
    // First run — check if gh CLI is available
    const ghAvailable = isGhAvailable();
    if (!ghAvailable) {
      log("info", "settings", "gh CLI not found — PR status disabled. Enable in settings if you install it later.");
    }
    return { ...DEFAULT_SETTINGS, ghPrStatus: ghAvailable };
  }
  try {
    const raw = readFileSync(SETTINGS_PATH, "utf-8");
    const parsed = JSON.parse(raw);
    return { ...DEFAULT_SETTINGS, ...parsed };
  } catch {
    return { ...DEFAULT_SETTINGS };
  }
}

export function saveSettings(settings: Settings): void {
  if (!existsSync(APP_DIR)) {
    mkdirSync(APP_DIR, { recursive: true });
  }
  writeFileSync(SETTINGS_PATH, JSON.stringify(settings, null, 2) + "\n");
}
