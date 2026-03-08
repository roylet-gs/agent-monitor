import { describe, it, expect, vi, beforeEach } from "vitest";
import { mkdirSync, writeFileSync, readFileSync } from "fs";

vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

vi.mock("../../src/lib/hooks-installer.js", () => ({
  readGlobalSettings: vi.fn(() => ({})),
  writeGlobalSettings: vi.fn(),
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
