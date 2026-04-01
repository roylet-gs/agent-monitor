import { existsSync, mkdirSync, readFileSync, writeFileSync } from "fs";
import { dirname } from "path";
import { SETTINGS_PATH, APP_DIR } from "./paths.js";
import { isGhAvailable } from "./github.js";
import { log } from "./logger.js";
import type { Settings } from "./types.js";

export function isFirstRun(): boolean {
  if (!existsSync(SETTINGS_PATH)) return true;
  try {
    const raw = readFileSync(SETTINGS_PATH, "utf-8");
    const parsed = JSON.parse(raw);
    return !parsed.setupCompleted;
  } catch {
    return true;
  }
}

export const DEFAULT_SETTINGS: Settings = {
  ide: "cursor",
  defaultBranchPrefix: "feature/",
  defaultBaseBranch: "main",
  pollingIntervalMs: 30000,
  autoSyncOnStartup: true,
  compactView: false,
  hideMainBranch: true,
  audioNotifications: false,
  audioWaitingSound: "Glass",
  audioDoneSound: "Funk",
  ghPrStatus: true,
  ghPollingIntervalMs: 180000,
  logLevel: "info",
  linearEnabled: false,
  linearUseDesktopApp: false,
  linearApiKey: "",
  linearPollingIntervalMs: 180000,
  ghRefreshOnManual: true,
  linearRefreshOnManual: true,
  linearAutoNickname: true,
  maxLogSizeMb: 2,
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
  } catch (err) {
    log("warn", "settings", `Failed to parse settings file: ${err}`);
    return { ...DEFAULT_SETTINGS };
  }
}

export function saveSettings(settings: Settings): void {
  if (!existsSync(APP_DIR)) {
    mkdirSync(APP_DIR, { recursive: true });
  }
  writeFileSync(SETTINGS_PATH, JSON.stringify(settings, null, 2) + "\n");
}
