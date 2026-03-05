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
});
