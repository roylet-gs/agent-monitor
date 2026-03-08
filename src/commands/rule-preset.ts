import { listPresets, getPreset } from "../lib/presets.js";
import { enablePreset, disablePreset, isPresetEnabled } from "../lib/rules.js";
import { loadSettings, saveSettings } from "../lib/settings.js";
import { outputJson, outputTable } from "../lib/output.js";

export function presetList(opts: { json?: boolean }): void {
  const presets = listPresets();

  const data = presets.map((p) => ({
    id: p.id,
    name: p.name,
    description: p.description,
    enabled: isPresetEnabled(p.id),
    ruleCount: p.rules.length,
  }));

  if (opts.json) {
    outputJson(data);
    return;
  }

  if (data.length === 0) {
    console.log("No presets available.");
    return;
  }

  outputTable(
    data.map((d) => ({
      name: d.name,
      id: d.id,
      status: d.enabled ? "enabled" : "disabled",
      rules: String(d.ruleCount),
      description: d.description,
    })),
    [
      { key: "name", header: "Name" },
      { key: "id", header: "ID" },
      { key: "status", header: "Status" },
      { key: "rules", header: "Rules" },
      { key: "description", header: "Description" },
    ]
  );
}

export function presetEnable(name: string, opts: { json?: boolean }): void {
  const preset = getPreset(name);
  if (!preset) {
    if (opts.json) {
      outputJson({ error: `Unknown preset: ${name}` });
    } else {
      console.error(`Unknown preset: "${name}". Use \`am rule preset list\` to see available presets.`);
    }
    process.exitCode = 1;
    return;
  }

  const result = enablePreset(name);

  const settings = loadSettings();
  saveSettings({ ...settings, safeCommandsPresetEnabled: true });

  if (opts.json) {
    outputJson({ enabled: true, preset: name, added: result.added });
  } else {
    console.log(`Enabled "${preset.name}" preset: ${result.added} rule(s) added.`);
  }
}

export function presetDisable(name: string, opts: { json?: boolean }): void {
  const preset = getPreset(name);
  if (!preset) {
    if (opts.json) {
      outputJson({ error: `Unknown preset: ${name}` });
    } else {
      console.error(`Unknown preset: "${name}". Use \`am rule preset list\` to see available presets.`);
    }
    process.exitCode = 1;
    return;
  }

  const result = disablePreset(name);

  const settings = loadSettings();
  saveSettings({ ...settings, safeCommandsPresetEnabled: false });

  if (opts.json) {
    outputJson({ disabled: true, preset: name, removed: result.removed });
  } else {
    console.log(`Disabled "${preset.name}" preset: ${result.removed} rule(s) removed.`);
  }
}
