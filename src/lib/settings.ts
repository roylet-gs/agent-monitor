import { existsSync, mkdirSync, readFileSync, writeFileSync } from "fs";
import { dirname } from "path";
import { SETTINGS_PATH, APP_DIR } from "./paths.js";
import type { Settings } from "./types.js";

const DEFAULT_SETTINGS: Settings = {
  ide: "cursor",
  defaultBranchPrefix: "feature/",
  pollingIntervalMs: 2000,
  autoInstallHooks: true,
  autoSyncOnStartup: true,
  logLevel: "info",
};

export function loadSettings(): Settings {
  if (!existsSync(SETTINGS_PATH)) {
    return { ...DEFAULT_SETTINGS };
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
