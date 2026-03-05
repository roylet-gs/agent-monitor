import { loadSettings } from "../../lib/settings.js";
import { outputJson } from "../../lib/output.js";
import type { Settings } from "../../lib/types.js";

export function settingsGet(key: string, opts: { json?: boolean }): void {
  const settings = loadSettings();

  if (!(key in settings)) {
    console.error(`Unknown setting: ${key}`);
    console.error(`Valid keys: ${Object.keys(settings).join(", ")}`);
    process.exit(1);
  }

  const value = settings[key as keyof Settings];

  if (opts.json) {
    outputJson({ key, value });
  } else {
    console.log(String(value));
  }
}
