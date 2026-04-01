import { existsSync, mkdirSync, readFileSync, writeFileSync } from "fs";
import { join } from "path";
import { homedir } from "os";
import { log } from "./logger.js";

interface HookEntry {
  type: string;
  command: string;
  timeout: number;
}

interface HookMatcher {
  matcher?: string;
  hooks: HookEntry[];
}

type HooksConfig = Record<string, HookMatcher[]>;

const HOOK_EVENTS = ["PreToolUse", "PostToolUse", "Stop", "Notification", "SessionStart", "SessionEnd", "UserPromptSubmit", "SubagentStart", "SubagentStop"] as const;

// PermissionRequest is only needed for managed mode
const MANAGED_EXTRA_EVENTS = ["PermissionRequest"] as const;

// Events that should use the managed mode blocking bridge (longer timeout + --managed flag)
const MANAGED_BLOCKING_EVENTS = new Set(["PreToolUse", "PermissionRequest"]);

const HOOK_MARKER = "am hook-event";

function buildHookEntry(event: string, managedMode = false): HookMatcher {
  const useManaged = managedMode && MANAGED_BLOCKING_EVENTS.has(event);
  const managedFlag = useManaged ? " --managed" : "";
  const timeout = useManaged ? 300000 : 5000; // 5 min for managed blocking, 5s otherwise

  return {
    hooks: [
      {
        type: "command",
        command: `cat | am hook-event --worktree "$CLAUDE_PROJECT_DIR" --event ${event}${managedFlag}`,
        timeout,
      },
    ],
  };
}

function getGlobalSettingsPath(): string {
  return join(homedir(), ".claude", "settings.json");
}

export function readGlobalSettings(): Record<string, unknown> {
  const settingsPath = getGlobalSettingsPath();
  if (!existsSync(settingsPath)) return {};
  try {
    return JSON.parse(readFileSync(settingsPath, "utf-8"));
  } catch {
    return {};
  }
}

export function writeGlobalSettings(settings: Record<string, unknown>): void {
  const claudeDir = join(homedir(), ".claude");
  if (!existsSync(claudeDir)) {
    mkdirSync(claudeDir, { recursive: true });
  }
  writeFileSync(getGlobalSettingsPath(), JSON.stringify(settings, null, 2) + "\n");
}

function hasAmHook(matchers: HookMatcher[]): boolean {
  return matchers.some((m) =>
    m.hooks?.some((h) => h.command?.includes(HOOK_MARKER))
  );
}

export function installGlobalHooks(managedMode = false): void {
  const settings = readGlobalSettings();
  const hooks = (settings.hooks ?? {}) as HooksConfig;

  const allEvents = managedMode
    ? [...HOOK_EVENTS, ...MANAGED_EXTRA_EVENTS]
    : [...HOOK_EVENTS];

  for (const event of allEvents) {
    const existing = hooks[event] ?? [];
    if (!hasAmHook(existing)) {
      hooks[event] = [...existing, buildHookEntry(event, managedMode)];
    }
  }

  settings.hooks = hooks;
  writeGlobalSettings(settings);
  log("info", "hooks", `Installed global hooks into ${getGlobalSettingsPath()} (managedMode=${managedMode})`);
}

export function uninstallGlobalHooks(): void {
  const settings = readGlobalSettings();
  const hooks = (settings.hooks ?? {}) as HooksConfig;

  const allEvents = [...HOOK_EVENTS, ...MANAGED_EXTRA_EVENTS];
  for (const event of allEvents) {
    const existing = hooks[event];
    if (!existing) continue;
    hooks[event] = existing.filter(
      (m) => !m.hooks?.some((h) => h.command?.includes(HOOK_MARKER))
    );
    if (hooks[event].length === 0) {
      delete hooks[event];
    }
  }

  // Remove hooks key entirely if empty
  if (Object.keys(hooks).length === 0) {
    delete settings.hooks;
  } else {
    settings.hooks = hooks;
  }

  writeGlobalSettings(settings);
  log("info", "hooks", `Uninstalled global hooks from ${getGlobalSettingsPath()}`);
}

/**
 * Reinstall hooks with the correct timeout/flags for managed mode.
 * Removes existing AM hooks first, then installs fresh.
 */
export function reinstallHooksForManagedMode(enabled: boolean): void {
  uninstallGlobalHooks();
  installGlobalHooks(enabled);
  log("info", "hooks", `Reinstalled hooks for managed mode: ${enabled ? "ON" : "OFF"}`);
}

export function isGlobalHooksInstalled(): boolean {
  const settings = readGlobalSettings();
  const hooks = (settings.hooks ?? {}) as HooksConfig;
  return HOOK_EVENTS.some((event) => {
    const existing = hooks[event];
    return existing && hasAmHook(existing);
  });
}
