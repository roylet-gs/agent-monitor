import { loadSettings, saveSettings, DEFAULT_SETTINGS } from "../../lib/settings.js";
import type { Settings } from "../../lib/types.js";

export function settingsSet(key: string, value: string): void {
  const settings = loadSettings();

  if (!(key in settings)) {
    console.error(`Unknown setting: ${key}`);
    console.error(`Valid keys: ${Object.keys(settings).join(", ")}`);
    process.exit(1);
  }

  // The sort list is an ordered array of objects that can't be expressed on the
  // command line. Point users at the places that can edit it.
  if (key === "worktreeSort") {
    console.error(
      "worktreeSort is a list and can't be set from the CLI.\n" +
        "Edit it in the TUI (Settings → Sorting & Display → Sort Order) or\n" +
        "run `am` and use the \"Open settings.json\" action to edit it directly."
    );
    process.exit(1);
  }

  // Type-coerce based on the default value type
  const defaultVal = DEFAULT_SETTINGS[key as keyof Settings];
  let coerced: unknown;

  if (typeof defaultVal === "boolean") {
    if (value === "true" || value === "1") coerced = true;
    else if (value === "false" || value === "0") coerced = false;
    else {
      console.error(`Invalid boolean value: ${value}. Use true/false.`);
      process.exit(1);
    }
  } else if (typeof defaultVal === "number") {
    coerced = Number(value);
    if (isNaN(coerced as number)) {
      console.error(`Invalid number value: ${value}`);
      process.exit(1);
    }
  } else {
    coerced = value;
  }

  (settings as unknown as Record<string, unknown>)[key] = coerced;
  saveSettings(settings);
  console.log(`${key} = ${coerced}`);
}
