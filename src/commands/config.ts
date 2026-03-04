import { loadSettings, saveSettings, DEFAULT_SETTINGS } from "../lib/settings.js";
import type { Settings } from "../lib/types.js";

const VALID_KEYS = Object.keys(DEFAULT_SETTINGS) as (keyof Settings)[];

export function runConfig(args: string[], flags: { json?: boolean; reset?: boolean }): void {
  if (flags.reset) {
    saveSettings({ ...DEFAULT_SETTINGS });
    console.log("Settings reset to defaults.");
    return;
  }

  const settings = loadSettings();
  const [key, ...valueParts] = args;

  // No args: list all
  if (!key) {
    if (flags.json) {
      console.log(JSON.stringify(settings, null, 2));
    } else {
      for (const k of VALID_KEYS) {
        const val = settings[k];
        console.log(`${k} = ${typeof val === "string" ? val : JSON.stringify(val)}`);
      }
    }
    return;
  }

  if (!VALID_KEYS.includes(key as keyof Settings)) {
    console.error(`Error: unknown setting "${key}". Valid keys: ${VALID_KEYS.join(", ")}`);
    process.exit(1);
  }

  const settingKey = key as keyof Settings;

  // One arg: get
  if (valueParts.length === 0) {
    const val = settings[settingKey];
    if (flags.json) {
      console.log(JSON.stringify({ [settingKey]: val }));
    } else {
      console.log(`${settingKey} = ${typeof val === "string" ? val : JSON.stringify(val)}`);
    }
    return;
  }

  // Two args: set
  const rawValue = valueParts.join(" ");
  const defaultVal = DEFAULT_SETTINGS[settingKey];

  let parsed: unknown;
  if (typeof defaultVal === "boolean") {
    if (rawValue === "true") parsed = true;
    else if (rawValue === "false") parsed = false;
    else {
      console.error(`Error: "${settingKey}" expects true or false`);
      process.exit(1);
    }
  } else if (typeof defaultVal === "number") {
    parsed = Number(rawValue);
    if (isNaN(parsed as number)) {
      console.error(`Error: "${settingKey}" expects a number`);
      process.exit(1);
    }
  } else {
    parsed = rawValue;
  }

  const updated = { ...settings, [settingKey]: parsed };
  saveSettings(updated);
  console.log(`${settingKey} = ${typeof parsed === "string" ? parsed : JSON.stringify(parsed)}`);
}
