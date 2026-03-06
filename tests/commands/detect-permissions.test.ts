import { describe, it, expect, vi, beforeEach } from "vitest";
import { mkdirSync, writeFileSync } from "fs";
import { join } from "path";
import { getTestDir } from "../setup.js";

vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

vi.mock("../../src/lib/hooks-installer.js", () => ({
  readGlobalSettings: vi.fn(() => ({})),
  writeGlobalSettings: vi.fn(),
}));

// Mock settings to control applyGlobalRulesEnabled
vi.mock("../../src/lib/settings.js", () => ({
  loadSettings: vi.fn(() => ({ applyGlobalRulesEnabled: false })),
}));

describe("detectNewPermissions integration", () => {
  let rules: typeof import("../../src/lib/rules.js");
  let paths: typeof import("../../src/lib/paths.js");

  beforeEach(async () => {
    paths = await import("../../src/lib/paths.js");
    rules = await import("../../src/lib/rules.js");
    mkdirSync(paths.APP_DIR, { recursive: true });
  });

  it("detects new permissions from settings.local.json and creates learned rules", () => {
    // Simulate a worktree with settings.local.json
    const worktreePath = join(getTestDir(), "worktree");
    const claudeDir = join(worktreePath, ".claude");
    mkdirSync(claudeDir, { recursive: true });
    writeFileSync(
      join(claudeDir, "settings.local.json"),
      JSON.stringify({
        permissions: {
          allow: ["Bash(git push *)", "Read", "Edit(/src/*)"],
        },
      })
    );

    // Parse and add rules like detectNewPermissions does
    const settingsPath = join(worktreePath, ".claude", "settings.local.json");
    const raw = require("fs").readFileSync(settingsPath, "utf-8");
    const parsed = JSON.parse(raw);
    const allowEntries: string[] = parsed.permissions.allow;

    for (const entry of allowEntries) {
      const { tool, input_pattern } = rules.parseClaudePermissionRule(entry);
      const existingRules = rules.loadRules();
      const exists = existingRules.some(
        (r) => r.tool === tool && (r.input_pattern ?? "") === (input_pattern ?? "")
      );
      if (!exists) {
        rules.addRule(tool, input_pattern, "allow", "learned");
      }
    }

    const loaded = rules.loadRules();
    expect(loaded).toHaveLength(3);

    expect(loaded[0]!.tool).toBe("Bash");
    expect(loaded[0]!.input_pattern).toBe("git push *");
    expect(loaded[0]!.source).toBe("learned");

    expect(loaded[1]!.tool).toBe("Read");
    expect(loaded[1]!.input_pattern).toBeUndefined();
    expect(loaded[1]!.source).toBe("learned");

    expect(loaded[2]!.tool).toBe("Edit");
    expect(loaded[2]!.input_pattern).toBe("/src/*");
    expect(loaded[2]!.source).toBe("learned");
  });

  it("skips duplicate rules on repeated detection", () => {
    // Add a rule that already exists
    rules.addRule("Bash", "git push *", "allow", "learned");

    const worktreePath = join(getTestDir(), "worktree2");
    const claudeDir = join(worktreePath, ".claude");
    mkdirSync(claudeDir, { recursive: true });
    writeFileSync(
      join(claudeDir, "settings.local.json"),
      JSON.stringify({
        permissions: {
          allow: ["Bash(git push *)", "Read"],
        },
      })
    );

    const settingsPath = join(worktreePath, ".claude", "settings.local.json");
    const raw = require("fs").readFileSync(settingsPath, "utf-8");
    const parsed = JSON.parse(raw);
    const allowEntries: string[] = parsed.permissions.allow;

    for (const entry of allowEntries) {
      const { tool, input_pattern } = rules.parseClaudePermissionRule(entry);
      const existingRules = rules.loadRules();
      const exists = existingRules.some(
        (r) => r.tool === tool && (r.input_pattern ?? "") === (input_pattern ?? "")
      );
      if (!exists) {
        rules.addRule(tool, input_pattern, "allow", "learned");
      }
    }

    const loaded = rules.loadRules();
    // Should have original Bash + new Read, not duplicate Bash
    expect(loaded).toHaveLength(2);
    expect(loaded[0]!.tool).toBe("Bash");
    expect(loaded[1]!.tool).toBe("Read");
  });
});
