import {
  loadRules,
  addRule,
  removeRule,
  clearRules,
  applyRulesToClaudeSettings,
  removeAmPermissionsFromClaudeSettings,
} from "../lib/rules.js";
import { loadSettings } from "../lib/settings.js";
import { outputJson, outputTable } from "../lib/output.js";

function autoApplyIfEnabled(): void {
  const settings = loadSettings();
  if (settings.applyGlobalRulesEnabled) {
    const result = applyRulesToClaudeSettings();
    console.log(`Applied ${result.added} rule(s) to ~/.claude/settings.json (${result.total} total permissions)`);
  }
}

export function ruleList(opts: { json?: boolean }): void {
  const rules = loadRules();

  if (opts.json) {
    outputJson(rules);
    return;
  }

  if (rules.length === 0) {
    console.log("No rules configured. Use `am rule add <tool>` to add one.");
    return;
  }

  outputTable(
    rules.map((r) => ({
      id: r.id.slice(0, 8),
      tool: r.tool,
      pattern: r.input_pattern ?? "",
      decision: r.decision,
    })),
    [
      { key: "id", header: "ID" },
      { key: "tool", header: "Tool" },
      { key: "pattern", header: "Pattern" },
      { key: "decision", header: "Decision" },
    ]
  );
}

export function ruleAdd(
  tool: string,
  opts: { input?: string; deny?: boolean; json?: boolean }
): void {
  const decision = opts.deny ? "deny" : "allow";
  const rule = addRule(tool, opts.input, decision as "allow" | "deny");

  if (opts.json) {
    outputJson(rule);
  } else {
    console.log(`Added rule: ${rule.tool}${rule.input_pattern ? ` (${rule.input_pattern})` : ""} → ${rule.decision} [${rule.id.slice(0, 8)}]`);
  }

  autoApplyIfEnabled();
}

export function ruleRemove(
  idOrTool: string,
  opts: { json?: boolean }
): void {
  const removed = removeRule(idOrTool);

  if (opts.json) {
    outputJson(removed ? { removed: true, rule: removed } : { removed: false });
  } else if (!removed) {
    console.error(`No rule found matching "${idOrTool}"`);
    process.exitCode = 1;
    return;
  } else {
    console.log(`Removed rule: ${removed.tool}${removed.input_pattern ? ` (${removed.input_pattern})` : ""} [${removed.id.slice(0, 8)}]`);
  }

  autoApplyIfEnabled();
}

export function ruleClear(opts: { json?: boolean }): void {
  const result = clearRules();

  if (opts.json) {
    outputJson(result);
  } else if (result.removed === 0) {
    console.log("No rules to clear.");
  } else {
    console.log(`Cleared ${result.removed} rule${result.removed === 1 ? "" : "s"}.`);
  }

  const settings = loadSettings();
  if (settings.applyGlobalRulesEnabled) {
    removeAmPermissionsFromClaudeSettings();
    if (!opts.json) {
      console.log("Restored ~/.claude/settings.json to baseline.");
    }
  }
}

export function ruleApply(opts: { json?: boolean }): void {
  const result = applyRulesToClaudeSettings();

  if (opts.json) {
    outputJson(result);
    return;
  }

  if (result.added === 0) {
    console.log("No rules to apply.");
  } else {
    console.log(`Applied ${result.added} rule(s) to ~/.claude/settings.json (${result.total} total permissions)`);
  }
}

export function ruleRestore(opts: { json?: boolean }): void {
  removeAmPermissionsFromClaudeSettings();

  if (opts.json) {
    outputJson({ restored: true });
    return;
  }

  console.log("Removed all am-managed permissions from ~/.claude/settings.json");
}
