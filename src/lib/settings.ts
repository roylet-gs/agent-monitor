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
  // The first three enabled criteria reproduce the legacy hardcoded ordering:
  // dedicated worktrees first / main last, Linear ticket clustering, newest first.
  worktreeSort: [
    { key: "isMain", direction: "asc", enabled: true },
    { key: "linearTicket", direction: "asc", enabled: true },
    { key: "createdAt", direction: "desc", enabled: true },
    { key: "repo", direction: "asc", enabled: false },
    { key: "agentStatus", direction: "asc", enabled: false },
    { key: "lastActivity", direction: "desc", enabled: false },
    { key: "linearProject", direction: "asc", enabled: false },
    { key: "prStatus", direction: "asc", enabled: false },
    { key: "gitDirty", direction: "asc", enabled: false },
    { key: "branchName", direction: "asc", enabled: false },
  ],
  hideMergedClosedPrs: false,
  hideIdleDoneAgents: false,
  hideWithoutLinearTicket: false,
  showPrStatus: true,
  showLinearTicket: true,
  showGitAheadBehind: true,
  showLastCommit: true,
  showRunningProcesses: false,
  runningProcessFilter: "",
  worktreeLimitEnabled: false,
  maxWorktrees: 5,
  maxLogSizeMb: 2,
  agentPermissionMode: "acceptEdits",
  agentClaudeArgs: "",
  resumeLastSession: true,
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
