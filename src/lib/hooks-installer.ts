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
  matcher: string;
  hooks: HookEntry[];
}

type HooksConfig = Record<string, HookMatcher[]>;

const HOOK_EVENTS = ["PreToolUse", "PostToolUse", "Stop", "Notification", "SessionStart"] as const;

const HOOK_MARKER = "am hook-event";

function buildHookEntry(event: string): HookMatcher {
  return {
    matcher: "",
    hooks: [
      {
        type: "command",
        command: `cat | am hook-event --worktree "$CLAUDE_PROJECT_DIR" --event ${event}`,
        timeout: 5000,
      },
    ],
  };
}

function getGlobalSettingsPath(): string {
  return join(homedir(), ".claude", "settings.json");
}

function readGlobalSettings(): Record<string, unknown> {
  const settingsPath = getGlobalSettingsPath();
  if (!existsSync(settingsPath)) return {};
  try {
    return JSON.parse(readFileSync(settingsPath, "utf-8"));
  } catch {
    return {};
  }
}

function writeGlobalSettings(settings: Record<string, unknown>): void {
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

export function installGlobalHooks(): void {
  const settings = readGlobalSettings();
  const hooks = (settings.hooks ?? {}) as HooksConfig;

  for (const event of HOOK_EVENTS) {
    const existing = hooks[event] ?? [];
    if (!hasAmHook(existing)) {
      hooks[event] = [...existing, buildHookEntry(event)];
    }
  }

  settings.hooks = hooks;
  writeGlobalSettings(settings);
  log("info", "hooks", `Installed global hooks into ${getGlobalSettingsPath()}`);
}

export function uninstallGlobalHooks(): void {
  const settings = readGlobalSettings();
  const hooks = (settings.hooks ?? {}) as HooksConfig;

  for (const event of HOOK_EVENTS) {
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

export function isGlobalHooksInstalled(): boolean {
  const settings = readGlobalSettings();
  const hooks = (settings.hooks ?? {}) as HooksConfig;
  return HOOK_EVENTS.some((event) => {
    const existing = hooks[event];
    return existing && hasAmHook(existing);
  });
}
