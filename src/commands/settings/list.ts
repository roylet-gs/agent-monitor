import { loadSettings } from "../../lib/settings.js";
import { outputJson, outputKeyValue } from "../../lib/output.js";

export function settingsList(opts: { json?: boolean }): void {
  const settings = loadSettings();

  if (opts.json) {
    outputJson(settings);
    return;
  }

  const pairs: [string, string][] = Object.entries(settings).map(([k, v]) => [k, String(v)]);
  outputKeyValue(pairs);
}
