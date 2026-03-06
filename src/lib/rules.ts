import { existsSync, readFileSync, writeFileSync, mkdirSync } from "fs";
import { randomUUID } from "crypto";
import { RULES_PATH, AM_MANAGED_PERMISSIONS_PATH, APP_DIR } from "./paths.js";
import { readGlobalSettings, writeGlobalSettings } from "./hooks-installer.js";
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
  decision: "allow" | "deny" = "allow"
): Rule {
  const rules = loadRules();
  const rule: Rule = {
    id: randomUUID(),
    tool,
    ...(inputPattern ? { input_pattern: inputPattern } : {}),
    decision,
    source: "manual",
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

export function clearRules(): { removed: number } {
  const rules = loadRules();
  const removed = rules.length;
  if (removed > 0) {
    saveRules([]);
    log("info", "rules", `Cleared all ${removed} rule(s)`);
  }
  return { removed };
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

