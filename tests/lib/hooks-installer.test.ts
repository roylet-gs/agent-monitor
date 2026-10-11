import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { readFileSync, writeFileSync, mkdirSync, existsSync } from "fs";
import { join } from "path";
import { tmpdir } from "os";
import { mkdtempSync, rmSync } from "fs";

vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

describe("hooks-installer", () => {
  let tempHome: string;
  let originalHome: string;

  beforeEach(() => {
    tempHome = mkdtempSync(join(tmpdir(), "am-hooks-test-"));
    originalHome = process.env.HOME!;
    // hooks-installer uses homedir() which reads HOME env var
    process.env.HOME = tempHome;
  });

  afterEach(() => {
    process.env.HOME = originalHome;
    rmSync(tempHome, { recursive: true, force: true });
  });

  it("installs global hooks", async () => {
    const { installGlobalHooks, isGlobalHooksInstalled } = await import(
      "../../src/lib/hooks-installer.js"
    );
    installGlobalHooks();
    expect(isGlobalHooksInstalled()).toBe(true);

    const settingsPath = join(tempHome, ".claude", "settings.json");
    expect(existsSync(settingsPath)).toBe(true);
    const settings = JSON.parse(readFileSync(settingsPath, "utf-8"));
    expect(settings.hooks).toBeDefined();
    expect(settings.hooks.Stop).toBeDefined();
    expect(settings.hooks.PreToolUse).toBeDefined();
  });

  it("does not duplicate hooks on re-install", async () => {
    const { installGlobalHooks } = await import("../../src/lib/hooks-installer.js");
    installGlobalHooks();
    installGlobalHooks();

    const settingsPath = join(tempHome, ".claude", "settings.json");
    const settings = JSON.parse(readFileSync(settingsPath, "utf-8"));
    // Each event should have exactly 1 matcher
    expect(settings.hooks.Stop).toHaveLength(1);
  });

  it("preserves existing hooks when installing", async () => {
    const claudeDir = join(tempHome, ".claude");
    mkdirSync(claudeDir, { recursive: true });
    writeFileSync(
      join(claudeDir, "settings.json"),
      JSON.stringify({
        hooks: {
          PreToolUse: [
            { matcher: "custom", hooks: [{ type: "command", command: "echo custom", timeout: 1000 }] },
          ],
        },
      })
    );

    const { installGlobalHooks } = await import("../../src/lib/hooks-installer.js");
    installGlobalHooks();

    const settings = JSON.parse(readFileSync(join(claudeDir, "settings.json"), "utf-8"));
    // Should have both custom and am hooks
    expect(settings.hooks.PreToolUse).toHaveLength(2);
  });

  it("uninstalls global hooks", async () => {
    const { installGlobalHooks, uninstallGlobalHooks, isGlobalHooksInstalled } = await import(
      "../../src/lib/hooks-installer.js"
    );
    installGlobalHooks();
    expect(isGlobalHooksInstalled()).toBe(true);
    uninstallGlobalHooks();
    expect(isGlobalHooksInstalled()).toBe(false);
  });

  it("uninstall preserves non-am hooks", async () => {
    const claudeDir = join(tempHome, ".claude");
    mkdirSync(claudeDir, { recursive: true });
    writeFileSync(
      join(claudeDir, "settings.json"),
      JSON.stringify({
        hooks: {
          PreToolUse: [
            { matcher: "custom", hooks: [{ type: "command", command: "echo custom", timeout: 1000 }] },
          ],
        },
      })
    );

    const { installGlobalHooks, uninstallGlobalHooks } = await import(
      "../../src/lib/hooks-installer.js"
    );
    installGlobalHooks();
    uninstallGlobalHooks();

    const settings = JSON.parse(readFileSync(join(claudeDir, "settings.json"), "utf-8"));
    expect(settings.hooks.PreToolUse).toHaveLength(1);
    expect(settings.hooks.PreToolUse[0].matcher).toBe("custom");
  });

  it("isGlobalHooksInstalled returns false when no settings", async () => {
    const { isGlobalHooksInstalled } = await import("../../src/lib/hooks-installer.js");
    expect(isGlobalHooksInstalled()).toBe(false);
  });

  it("installs PermissionRequest hook with --managed and 300000ms timeout", async () => {
    const { installGlobalHooks } = await import("../../src/lib/hooks-installer.js");
    installGlobalHooks();

    const settings = JSON.parse(
      readFileSync(join(tempHome, ".claude", "settings.json"), "utf-8")
    );
    expect(settings.hooks.PermissionRequest).toBeDefined();
    expect(settings.hooks.PermissionRequest).toHaveLength(1);
    const entry = settings.hooks.PermissionRequest[0].hooks[0];
    expect(entry.command).toContain("--managed");
    expect(entry.command).toContain("--event PermissionRequest");
    expect(entry.timeout).toBe(300000);
  });

  it("non-managed events keep 5000ms timeout and no --managed flag", async () => {
    const { installGlobalHooks } = await import("../../src/lib/hooks-installer.js");
    installGlobalHooks();

    const settings = JSON.parse(
      readFileSync(join(tempHome, ".claude", "settings.json"), "utf-8")
    );
    const preToolUse = settings.hooks.PreToolUse[0].hooks[0];
    expect(preToolUse.command).not.toContain("--managed");
    expect(preToolUse.timeout).toBe(5000);
  });

  it("uninstall removes PermissionRequest hook", async () => {
    const { installGlobalHooks, uninstallGlobalHooks } = await import(
      "../../src/lib/hooks-installer.js"
    );
    installGlobalHooks();
    uninstallGlobalHooks();

    const settings = JSON.parse(
      readFileSync(join(tempHome, ".claude", "settings.json"), "utf-8")
    );
    expect(settings.hooks?.PermissionRequest).toBeUndefined();
  });
});

describe("hooks-installer native binary", () => {
  let tempHome: string;
  let originalHome: string;
  let fakeBin: string;

  beforeEach(() => {
    tempHome = mkdtempSync(join(tmpdir(), "am-hooks-native-"));
    originalHome = process.env.HOME!;
    process.env.HOME = tempHome;
    fakeBin = join(tempHome, "am-hook");
    writeFileSync(fakeBin, "#!/bin/sh\n", { mode: 0o755 });
  });

  afterEach(() => {
    process.env.HOME = originalHome;
    delete process.env.AM_HOOK_BIN;
    delete process.env.AM_NO_NATIVE_HOOK;
    rmSync(tempHome, { recursive: true, force: true });
  });

  const readHooks = () =>
    JSON.parse(readFileSync(join(tempHome, ".claude", "settings.json"), "utf-8")).hooks;

  it("uses the native binary without the cat shim when available", async () => {
    process.env.AM_HOOK_BIN = fakeBin;
    const { installGlobalHooks, isGlobalHooksInstalled } = await import("../../src/lib/hooks-installer.js");
    installGlobalHooks();
    const cmd: string = readHooks().PreToolUse[0].hooks[0].command;
    expect(cmd).toBe(`"${fakeBin}" hook-event --worktree "$CLAUDE_PROJECT_DIR" --event PreToolUse # am hook-event (native)`);
    // Old releases detect hooks by this literal; it must stay in the command.
    expect(cmd).toContain("am hook-event");
    expect(isGlobalHooksInstalled()).toBe(true);
  });

  it("re-install upgrades existing TS entries to the native binary in place", async () => {
    const { installGlobalHooks } = await import("../../src/lib/hooks-installer.js");
    process.env.AM_NO_NATIVE_HOOK = "1";
    installGlobalHooks();
    expect(readHooks().Stop[0].hooks[0].command).toContain("cat | am hook-event");

    delete process.env.AM_NO_NATIVE_HOOK;
    process.env.AM_HOOK_BIN = fakeBin;
    installGlobalHooks();
    const stop = readHooks().Stop;
    expect(stop).toHaveLength(1);
    expect(stop[0].hooks[0].command).toContain("am-hook\" hook-event");
    expect(readHooks().PermissionRequest[0].hooks[0].command).toContain("--managed");
  });

  it("uninstall removes native entries", async () => {
    process.env.AM_HOOK_BIN = fakeBin;
    const { installGlobalHooks, uninstallGlobalHooks, isGlobalHooksInstalled } = await import(
      "../../src/lib/hooks-installer.js"
    );
    installGlobalHooks();
    uninstallGlobalHooks();
    expect(isGlobalHooksInstalled()).toBe(false);
  });
});
