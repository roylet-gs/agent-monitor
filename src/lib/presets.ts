export interface PresetDefinition {
  id: string;
  name: string;
  description: string;
  rules: { tool: string; input_pattern: string; decision: "allow" | "deny" }[];
}

const SAFE_COMMANDS_PRESET: PresetDefinition = {
  id: "safe-commands",
  name: "Safe Commands",
  description: "Auto-approve harmless read-only Bash commands (find, ls, cat, git log, grep, etc.)",
  rules: [
    { tool: "Bash", input_pattern: "command=find *", decision: "allow" },
    { tool: "Bash", input_pattern: "command=ls *", decision: "allow" },
    { tool: "Bash", input_pattern: "command=cat *", decision: "allow" },
    { tool: "Bash", input_pattern: "command=head *", decision: "allow" },
    { tool: "Bash", input_pattern: "command=tail *", decision: "allow" },
    { tool: "Bash", input_pattern: "command=wc *", decision: "allow" },
    { tool: "Bash", input_pattern: "command=which *", decision: "allow" },
    { tool: "Bash", input_pattern: "command=git log*", decision: "allow" },
    { tool: "Bash", input_pattern: "command=git diff*", decision: "allow" },
    { tool: "Bash", input_pattern: "command=git status*", decision: "allow" },
    { tool: "Bash", input_pattern: "command=git branch*", decision: "allow" },
    { tool: "Bash", input_pattern: "command=git show*", decision: "allow" },
    { tool: "Bash", input_pattern: "command=git remote*", decision: "allow" },
    { tool: "Bash", input_pattern: "command=git rev-parse*", decision: "allow" },
    { tool: "Bash", input_pattern: "command=grep *", decision: "allow" },
    { tool: "Bash", input_pattern: "command=rg *", decision: "allow" },
    { tool: "Bash", input_pattern: "command=sqlite3 *", decision: "allow" },
    { tool: "Bash", input_pattern: "command=cd * && find *", decision: "allow" },
    { tool: "Bash", input_pattern: "command=cd * && git *", decision: "allow" },
    { tool: "Bash", input_pattern: "command=cd * && ls *", decision: "allow" },
    { tool: "Bash", input_pattern: "command=cd * && cat *", decision: "allow" },
  ],
};

const PRESETS: Map<string, PresetDefinition> = new Map([
  [SAFE_COMMANDS_PRESET.id, SAFE_COMMANDS_PRESET],
]);

export function getPreset(id: string): PresetDefinition | undefined {
  return PRESETS.get(id);
}

export function listPresets(): PresetDefinition[] {
  return [...PRESETS.values()];
}
