import { existsSync, readFileSync, writeFileSync, mkdirSync } from "fs";
import { join } from "path";
import { tmpdir } from "os";
import { randomUUID } from "crypto";
import { RULES_PATH, AM_MANAGED_PERMISSIONS_PATH, APP_DIR } from "./paths.js";
import { readGlobalSettings, writeGlobalSettings } from "./hooks-installer.js";
import { getRepositories, getWorktrees } from "./db.js";
import { log } from "./logger.js";
import type { Rule } from "./types.js";

// --- Storage ---

export function loadRules(): Rule[] {
  if (!existsSync(RULES_PATH)) return [];
  try {
    const raw = readFileSync(RULES_PATH, "utf-8");
    return JSON.parse(raw);
  } catch (err) {
    log("warn", "rules", `Failed to parse rules file: ${err}`);
    return [];
  }
}

export function saveRules(rules: Rule[]): void {
  if (!existsSync(APP_DIR)) {
    mkdirSync(APP_DIR, { recursive: true });
  }
  writeFileSync(RULES_PATH, JSON.stringify(rules, null, 2) + "\n");
}

export function addRule(
  tool: string,
  inputPattern?: string,
  decision: "allow" | "deny" = "allow",
  source: "manual" | "learned" = "manual"
): Rule {
  const rules = loadRules();
  const rule: Rule = {
    id: randomUUID(),
    tool,
    ...(inputPattern ? { input_pattern: inputPattern } : {}),
    decision,
    source,
    created_at: new Date().toISOString(),
  };
  rules.push(rule);
  saveRules(rules);
  log("info", "rules", `Added rule: ${tool}${inputPattern ? ` (${inputPattern})` : ""} → ${decision}`);
  return rule;
}

export function removeRule(idOrTool: string): Rule | null {
  const rules = loadRules();

  // Match by id prefix first
  let idx = rules.findIndex((r) => r.id.startsWith(idOrTool));
  // Then try exact tool name match
  if (idx === -1) {
    idx = rules.findIndex((r) => r.tool === idOrTool);
  }

  if (idx === -1) return null;

  const [removed] = rules.splice(idx, 1);
  saveRules(rules);
  log("info", "rules", `Removed rule: ${removed!.tool} (${removed!.id.slice(0, 8)})`);
  return removed!;
}

// --- Claude permission format conversion ---

export function ruleToClaudePermission(rule: Rule): string | null {
  if (rule.tool === "*") {
    log("warn", "rules", `Skipping wildcard rule — no Claude equivalent`);
    return null;
  }
  if (rule.input_pattern) {
    return `${rule.tool}(${rule.input_pattern})`;
  }
  return rule.tool;
}

export function parseClaudePermissionRule(entry: string): { tool: string; input_pattern?: string } {
  const match = entry.match(/^(\w+)\((.+)\)$/);
  if (match) {
    return { tool: match[1]!, input_pattern: match[2]! };
  }
  return { tool: entry };
}

// --- Am-managed permissions tracking ---

export function loadAmManagedPermissions(): string[] {
  if (!existsSync(AM_MANAGED_PERMISSIONS_PATH)) return [];
  try {
    const raw = readFileSync(AM_MANAGED_PERMISSIONS_PATH, "utf-8");
    return JSON.parse(raw);
  } catch {
    return [];
  }
}

export function saveAmManagedPermissions(perms: string[]): void {
  if (!existsSync(APP_DIR)) {
    mkdirSync(APP_DIR, { recursive: true });
  }
  writeFileSync(AM_MANAGED_PERMISSIONS_PATH, JSON.stringify(perms, null, 2) + "\n");
}

// --- Core merge: push am rules into ~/.claude/settings.json ---

export function applyRulesToClaudeSettings(): { added: number; total: number } {
  const rules = loadRules();

  // Convert rules to Claude permission strings, separated by decision
  const allowPerms: string[] = [];
  const denyPerms: string[] = [];
  for (const rule of rules) {
    const perm = ruleToClaudePermission(rule);
    if (!perm) continue;
    if (rule.decision === "deny") {
      denyPerms.push(perm);
    } else {
      allowPerms.push(perm);
    }
  }

  const previousManaged = loadAmManagedPermissions();

  // Read current Claude settings
  const claudeSettings = readGlobalSettings();
  const permissions = (claudeSettings.permissions ?? {}) as Record<string, unknown>;
  const currentAllow: string[] = Array.isArray(permissions.allow) ? [...permissions.allow] : [];
  const currentDeny: string[] = Array.isArray(permissions.deny) ? [...permissions.deny] : [];

  // Remove previously am-managed entries to get user's baseline
  const managedSet = new Set(previousManaged);
  const baselineAllow = currentAllow.filter((p) => !managedSet.has(p));
  const baselineDeny = currentDeny.filter((p) => !managedSet.has(p));

  // Merge baseline + new am permissions, deduplicate
  const mergedAllow = [...new Set([...baselineAllow, ...allowPerms])];
  const mergedDeny = [...new Set([...baselineDeny, ...denyPerms])];

  // Write back
  permissions.allow = mergedAllow;
  permissions.deny = mergedDeny;
  claudeSettings.permissions = permissions;
  writeGlobalSettings(claudeSettings);

  // Save the new am-managed set (both allow and deny combined)
  const newManaged = [...allowPerms, ...denyPerms];
  saveAmManagedPermissions(newManaged);

  const added = allowPerms.length + denyPerms.length;
  log("info", "rules", `Applied ${added} permission(s) to Claude settings (allow=${allowPerms.length}, deny=${denyPerms.length})`);
  return { added, total: mergedAllow.length + mergedDeny.length };
}

// --- Remove am permissions from Claude settings ---

export function removeAmPermissionsFromClaudeSettings(): void {
  const previousManaged = loadAmManagedPermissions();
  if (previousManaged.length === 0) return;

  const managedSet = new Set(previousManaged);

  const claudeSettings = readGlobalSettings();
  const permissions = (claudeSettings.permissions ?? {}) as Record<string, unknown>;
  const currentAllow: string[] = Array.isArray(permissions.allow) ? [...permissions.allow] : [];
  const currentDeny: string[] = Array.isArray(permissions.deny) ? [...permissions.deny] : [];

  permissions.allow = currentAllow.filter((p) => !managedSet.has(p));
  permissions.deny = currentDeny.filter((p) => !managedSet.has(p));
  claudeSettings.permissions = permissions;
  writeGlobalSettings(claudeSettings);

  // Clear managed file
  saveAmManagedPermissions([]);

  log("info", "rules", `Removed ${previousManaged.length} am-managed permission(s) from Claude settings`);
}

// --- Learning ---

export function clearLearnedRules(): { removed: number } {
  const rules = loadRules();
  const manual = rules.filter((r) => r.source !== "learned");
  const removed = rules.length - manual.length;
  if (removed > 0) {
    saveRules(manual);
    log("info", "rules", `Cleared ${removed} learned rule(s)`);
  }
  return { removed };
}

export function clearAllRules(): { removed: number } {
  const rules = loadRules();
  const removed = rules.length;
  if (removed > 0) {
    saveRules([]);
    log("info", "rules", "Cleared all " + removed + " rule(s)");
  }
  return { removed };
}

export function syncRulesFromWorktrees(worktreePaths: string[]): { added: number } {
  const existingRules = loadRules();
  let added = 0;

  for (const wtPath of worktreePaths) {
    const settingsPath = join(wtPath, ".claude", "settings.local.json");
    if (!existsSync(settingsPath)) continue;

    try {
      const raw = readFileSync(settingsPath, "utf-8");
      const parsed = JSON.parse(raw);
      const allowEntries: string[] = parsed?.permissions?.allow ?? [];

      for (const entry of allowEntries) {
        const { tool, input_pattern } = parseClaudePermissionRule(entry);

        // Deduplicate: skip if same tool + input_pattern exists
        const exists = existingRules.some(
          (r) => r.tool === tool && (r.input_pattern ?? "") === (input_pattern ?? "")
        );
        if (exists) continue;

        const rule: Rule = {
          id: randomUUID(),
          tool,
          ...(input_pattern ? { input_pattern } : {}),
          decision: "allow",
          source: "learned",
          created_at: new Date().toISOString(),
        };
        existingRules.push(rule);
        added++;
        log("info", "rules", `Learned rule from ${wtPath}: ${tool}${input_pattern ? ` (${input_pattern})` : ""}`);
      }
    } catch (err) {
      log("debug", "rules", `Failed to read ${settingsPath}: ${err}`);
    }
  }

  if (added > 0) {
    saveRules(existingRules);
  }

  return { added };
}

// --- Diff generation ---

function formatRuleEntry(rule: Rule): string {
  const toolPart = rule.input_pattern ? `${rule.tool}(${rule.input_pattern})` : rule.tool;
  const pad = Math.max(1, 30 - toolPart.length);
  return `${toolPart}${" ".repeat(pad)}[${rule.source}]`;
}

export function generateRulesDiffFiles(): { leftPath: string; rightPath: string } {
  // Left side: aggregated worktree permissions
  const permissionSet = new Set<string>();
  const repos = getRepositories();
  for (const repo of repos) {
    const worktrees = getWorktrees(repo.id);
    for (const wt of worktrees) {
      const settingsPath = join(wt.path, ".claude", "settings.local.json");
      if (!existsSync(settingsPath)) continue;
      try {
        const raw = readFileSync(settingsPath, "utf-8");
        const parsed = JSON.parse(raw);
        const allowEntries: string[] = parsed?.permissions?.allow ?? [];
        for (const entry of allowEntries) permissionSet.add(entry);
      } catch {
        // Skip unreadable files
      }
    }
  }

  const sortedPermissions = [...permissionSet].sort();
  const leftContent = [
    "# Permissions from .claude/settings.local.json across tracked worktrees",
    "# (What Claude Code already allows per-worktree)",
    "",
    ...sortedPermissions,
    "",
  ].join("\n");

  // Right side: am rules
  const rules = loadRules();
  const sortedRules = [...rules].sort((a, b) => {
    const aKey = a.input_pattern ? `${a.tool}(${a.input_pattern})` : a.tool;
    const bKey = b.input_pattern ? `${b.tool}(${b.input_pattern})` : b.tool;
    return aKey.localeCompare(bKey);
  });
  const rightContent = [
    "# Auto-approval rules from ~/.agent-monitor/rules.json",
    "# (What am will write to ~/.claude/settings.json permissions)",
    "",
    ...sortedRules.map(formatRuleEntry),
    "",
  ].join("\n");

  const leftPath = join(tmpdir(), "am-worktree-permissions.txt");
  const rightPath = join(tmpdir(), "am-rules.txt");
  writeFileSync(leftPath, leftContent);
  writeFileSync(rightPath, rightContent);

  log("info", "rules", `Generated diff files: ${leftPath}, ${rightPath}`);
  return { leftPath, rightPath };
}
