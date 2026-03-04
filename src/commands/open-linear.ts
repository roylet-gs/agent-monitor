import open from "open";
import { loadSettings } from "../lib/settings.js";
import { fetchLinearInfo } from "../lib/linear.js";
import { resolveWorktree, type ResolveOptions } from "./_resolve.js";

export async function runOpenLinear(opts: ResolveOptions): Promise<void> {
  const settings = loadSettings();
  if (!settings.linearEnabled || !settings.linearApiKey) {
    console.error("Error: Linear integration is not configured. Enable it in settings.");
    process.exit(1);
  }

  const wt = resolveWorktree(opts);
  const info = await fetchLinearInfo(wt.branch, settings.linearApiKey);
  if (!info) {
    console.error(`No Linear issue found for branch "${wt.branch}"`);
    process.exit(1);
  }

  await open(info.url);
  console.log(`Opened ${info.identifier}: ${info.url}`);
}
