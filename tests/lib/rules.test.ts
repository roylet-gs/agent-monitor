import { describe, it, expect, vi, beforeEach } from "vitest";
import { mkdirSync, writeFileSync, readFileSync } from "fs";
import { join } from "path";

vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

vi.mock("../../src/lib/hooks-installer.js", () => ({
  readGlobalSettings: vi.fn(() => ({})),
  writeGlobalSettings: vi.fn(),
}));

vi.mock("../../src/lib/settings.js", () => ({
  loadSettings: vi.fn(() => ({ applyGlobalRulesEnabled: false, learnFromApprovalsEnabled: false })),
  saveSettings: vi.fn(),
  DEFAULT_SETTINGS: {},
}));

vi.mock("../../src/lib/db.js", () => ({
  getRepositories: vi.fn(() => []),
  getWorktrees: vi.fn(() => []),
}));

describe("rules", () => {
  let rules: typeof import("../../src/lib/rules.js");
  let paths: typeof import("../../src/lib/paths.js");

  beforeEach(async () => {
    paths = await import("../../src/lib/paths.js");
    rules = await import("../../src/lib/rules.js");
    mkdirSync(paths.APP_DIR, { recursive: true });
  });

  describe("addRule", () => {
    it("adds a rule with default source 'manual'", () => {
      const rule = rules.addRule("Bash");
      expect(rule.tool).toBe("Bash");
      expect(rule.decision).toBe("allow");
      expect(rule.source).toBe("manual");

      const loaded = rules.loadRules();
      expect(loaded).toHaveLength(1);
      expect(loaded[0]!.source).toBe("manual");
    });

    it("adds a rule with source 'learned'", () => {
      const rule = rules.addRule("Read", undefined, "allow", "learned");
      expect(rule.source).toBe("learned");

      const loaded = rules.loadRules();
      expect(loaded).toHaveLength(1);
      expect(loaded[0]!.source).toBe("learned");
    });

    it("adds a rule with input pattern", () => {
      const rule = rules.addRule("Bash", "git *", "allow", "learned");
      expect(rule.tool).toBe("Bash");
      expect(rule.input_pattern).toBe("git *");

      const loaded = rules.loadRules();
      expect(loaded[0]!.input_pattern).toBe("git *");
    });

    it("adds a deny rule", () => {
      const rule = rules.addRule("Write", undefined, "deny");
      expect(rule.decision).toBe("deny");
    });
  });

  describe("parseClaudePermissionRule", () => {
    it("parses simple tool name", () => {
      const result = rules.parseClaudePermissionRule("Bash");
      expect(result).toEqual({ tool: "Bash" });
    });

    it("parses tool with input pattern", () => {
      const result = rules.parseClaudePermissionRule("Bash(git push *)");
      expect(result).toEqual({ tool: "Bash", input_pattern: "git push *" });
    });

    it("parses tool with complex pattern", () => {
      const result = rules.parseClaudePermissionRule("Edit(/Users/test/file.ts)");
      expect(result).toEqual({ tool: "Edit", input_pattern: "/Users/test/file.ts" });
    });

    it("handles tool with nested parens in pattern", () => {
      const result = rules.parseClaudePermissionRule("Bash(echo (hello))");
      expect(result).toEqual({ tool: "Bash", input_pattern: "echo (hello)" });
    });
  });

  describe("clearAllRules", () => {
    it("clears all rules regardless of source", () => {
      rules.addRule("Bash", undefined, "allow", "manual");
      rules.addRule("Read", undefined, "allow", "learned");
      rules.addRule("Write", undefined, "deny", "manual");

      expect(rules.loadRules()).toHaveLength(3);

      const result = rules.clearAllRules();
      expect(result.removed).toBe(3);
      expect(rules.loadRules()).toHaveLength(0);
    });

    it("returns 0 when no rules exist", () => {
      const result = rules.clearAllRules();
      expect(result.removed).toBe(0);
    });
  });

  describe("removeRule", () => {
    it("removes by id prefix", () => {
      const rule = rules.addRule("Bash");
      const removed = rules.removeRule(rule.id.slice(0, 8));
      expect(removed).not.toBeNull();
      expect(removed!.tool).toBe("Bash");
      expect(rules.loadRules()).toHaveLength(0);
    });

    it("removes by tool name", () => {
      rules.addRule("Bash");
      const removed = rules.removeRule("Bash");
      expect(removed).not.toBeNull();
      expect(rules.loadRules()).toHaveLength(0);
    });

    it("returns null for non-existent rule", () => {
      expect(rules.removeRule("nonexistent")).toBeNull();
    });
  });

  describe("loadRules - migration", () => {
    it("filters out stale preset rules", () => {
      // Write rules file with a mix of sources including legacy "preset"
      const staleRules = [
        { id: "aaa", tool: "Bash", input_pattern: "ls *", decision: "allow", source: "preset", created_at: "2024-01-01" },
        { id: "bbb", tool: "Read", decision: "allow", source: "manual", created_at: "2024-01-01" },
        { id: "ccc", tool: "Bash", input_pattern: "cat *", decision: "allow", source: "preset", created_at: "2024-01-01" },
        { id: "ddd", tool: "Write", decision: "deny", source: "learned", created_at: "2024-01-01" },
      ];
      writeFileSync(paths.RULES_PATH, JSON.stringify(staleRules));

      const loaded = rules.loadRules();
      expect(loaded).toHaveLength(2);
      expect(loaded[0]!.tool).toBe("Read");
      expect(loaded[0]!.source).toBe("manual");
      expect(loaded[1]!.tool).toBe("Write");
      expect(loaded[1]!.source).toBe("learned");
    });
  });

  describe("syncLearnedRules", () => {
    it("learns rules from worktree settings.local.json files", async () => {
      const { getRepositories, getWorktrees } = vi.mocked(await import("../../src/lib/db.js"));

      // Create a fake worktree with settings.local.json
      const wtPath = join(paths.APP_DIR, "fake-worktree");
      const claudeDir = join(wtPath, ".claude");
      mkdirSync(claudeDir, { recursive: true });
      writeFileSync(join(claudeDir, "settings.local.json"), JSON.stringify({
        permissions: {
          allow: ["Bash(git status*)", "Read"],
          deny: ["Write"],
        },
      }));

      getRepositories.mockReturnValue([{ id: "repo1", path: "/tmp/repo", name: "test", last_used_at: "" }]);
      getWorktrees.mockReturnValue([{ id: "wt1", repo_id: "repo1", path: wtPath, branch: "main", name: "main", custom_name: null, nickname_source: null, is_main: 1, created_at: "" }]);

      const result = rules.syncLearnedRules();
      expect(result.added).toBe(3);

      const loaded = rules.loadRules();
      expect(loaded).toHaveLength(3);

      const allowRules = loaded.filter((r) => r.decision === "allow");
      expect(allowRules).toHaveLength(2);
      expect(allowRules.some((r) => r.tool === "Bash" && r.input_pattern === "git status*")).toBe(true);
      expect(allowRules.some((r) => r.tool === "Read" && !r.input_pattern)).toBe(true);

      const denyRules = loaded.filter((r) => r.decision === "deny");
      expect(denyRules).toHaveLength(1);
      expect(denyRules[0]!.tool).toBe("Write");

      expect(loaded.every((r) => r.source === "learned")).toBe(true);
    });

    it("does not duplicate existing rules", async () => {
      const { getRepositories, getWorktrees } = vi.mocked(await import("../../src/lib/db.js"));

      const wtPath = join(paths.APP_DIR, "fake-worktree2");
      const claudeDir = join(wtPath, ".claude");
      mkdirSync(claudeDir, { recursive: true });
      writeFileSync(join(claudeDir, "settings.local.json"), JSON.stringify({
        permissions: { allow: ["Bash"] },
      }));

      getRepositories.mockReturnValue([{ id: "repo1", path: "/tmp/repo", name: "test", last_used_at: "" }]);
      getWorktrees.mockReturnValue([{ id: "wt1", repo_id: "repo1", path: wtPath, branch: "main", name: "main", custom_name: null, nickname_source: null, is_main: 1, created_at: "" }]);

      // Add the rule manually first
      rules.addRule("Bash", undefined, "allow", "manual");

      const result = rules.syncLearnedRules();
      expect(result.added).toBe(0);
      expect(rules.loadRules()).toHaveLength(1);
    });
  });

  describe("clearLearnedRules", () => {
    it("removes only learned rules", () => {
      rules.addRule("Bash", undefined, "allow", "manual");
      rules.addRule("Read", undefined, "allow", "learned");
      rules.addRule("Write", undefined, "deny", "learned");

      const result = rules.clearLearnedRules();
      expect(result.removed).toBe(2);

      const remaining = rules.loadRules();
      expect(remaining).toHaveLength(1);
      expect(remaining[0]!.source).toBe("manual");
    });
  });

  describe("applyRulesToClaudeSettings", () => {
    it("applies rules to Claude settings", async () => {
      const { writeGlobalSettings } = await import("../../src/lib/hooks-installer.js") as { writeGlobalSettings: ReturnType<typeof vi.fn> };

      rules.addRule("Bash", undefined, "allow");
      rules.addRule("Write", undefined, "deny");

      const result = rules.applyRulesToClaudeSettings();
      expect(result.added).toBe(2);
      expect(writeGlobalSettings).toHaveBeenCalled();
    });
  });
});
