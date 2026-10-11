import { accessSync, constants, existsSync, mkdirSync, readFileSync, writeFileSync } from "fs";
import { delimiter, join } from "path";
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

const HOOK_EVENTS = ["PreToolUse", "PostToolUse", "Stop", "StopFailure", "Notification", "SessionStart", "SessionEnd", "UserPromptSubmit", "SubagentStart", "SubagentStop", "PermissionRequest"] as const;

// Both the TS command (`am hook-event`) and the native Rust binary
// (`"/path/to/am-hook" hook-event`, see crates/am-hook) count as "our" hooks.
const HOOK_MARKER = /\bam(-hook"?)?\s+hook-event\b/;

function isAmHookCommand(command: string | undefined): boolean {
  return !!command && HOOK_MARKER.test(command);
}

function isExecutable(path: string): boolean {
  try {
    accessSync(path, constants.X_OK);
    return true;
  } catch {
    return false;
  }
}

/**
 * Locate the native `am-hook` binary: `$AM_HOOK_BIN`, then `$PATH`, then
 * `~/.cargo/bin`. It starts in a few ms vs ~250 ms for `am hook-event`, which
 * Claude Code runs on every tool call. An absolute path is written into the
 * hook command because hooks don't necessarily inherit the user's shell PATH.
 */
export function findNativeHookBinary(): string | null {
  if (process.env.AM_NO_NATIVE_HOOK) return null;
  const candidates = [
    process.env.AM_HOOK_BIN,
    ...(process.env.PATH ?? "").split(delimiter).filter(Boolean).map((dir) => join(dir, "am-hook")),
    join(homedir(), ".cargo", "bin", "am-hook"),
  ];
  for (const candidate of candidates) {
    if (candidate && isExecutable(candidate)) return candidate;
  }
  return null;
}

// Events that coordinate with Claude Code's permission layer. Claude Code
// rewrites these entries to include --managed with a 5-minute timeout on its
// own; writing them in that shape up front keeps install idempotent so Claude
// Code doesn't need to modify settings.json after every upgrade.
const MANAGED_HOOK_EVENTS = new Set<string>(["PermissionRequest"]);

function buildHookEntry(event: string, nativeBin: string | null): HookMatcher {
  const isManaged = MANAGED_HOOK_EVENTS.has(event);
  const managedFlag = isManaged ? " --managed" : "";
  // The native binary reads stdin itself, so it doesn't need the `cat |` shim.
  const invocation = nativeBin ? `"${nativeBin}" hook-event` : "cat | am hook-event";
  // Older releases only recognise hooks containing the literal `am hook-event`;
  // without it they re-add a TS handler that races the native one. The trailing
  // shell comment keeps them seeing the native hook as already installed.
  const compat = nativeBin ? " # am hook-event (native)" : "";
  return {
    hooks: [
      {
        type: "command",
        command: `${invocation} --worktree "$CLAUDE_PROJECT_DIR" --event ${event}${managedFlag}${compat}`,
        timeout: isManaged ? 300000 : 5000,
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
    m.hooks?.some((h) => isAmHookCommand(h.command))
  );
}

function withoutAmHooks(matchers: HookMatcher[]): HookMatcher[] {
  return matchers.filter((m) => !m.hooks?.some((h) => isAmHookCommand(h.command)));
}

export function installGlobalHooks(): void {
  const settings = readGlobalSettings();
  const hooks = (settings.hooks ?? {}) as HooksConfig;

  const nativeBin = findNativeHookBinary();

  // Replace (rather than skip) existing am entries so re-running install
  // switches between the TS command and the native binary.
  for (const event of HOOK_EVENTS) {
    const existing = hooks[event] ?? [];
    hooks[event] = [...withoutAmHooks(existing), buildHookEntry(event, nativeBin)];
  }

  settings.hooks = hooks;
  writeGlobalSettings(settings);
  log("info", "hooks", `Installed global hooks into ${getGlobalSettingsPath()} using ${nativeBin ?? "am hook-event (TS)"}`);
}

export function uninstallGlobalHooks(): void {
  const settings = readGlobalSettings();
  const hooks = (settings.hooks ?? {}) as HooksConfig;

  for (const event of HOOK_EVENTS) {
    const existing = hooks[event];
    if (!existing) continue;
    hooks[event] = withoutAmHooks(existing);
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
