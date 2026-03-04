import { openInIde } from "../lib/ide-launcher.js";
import { loadSettings } from "../lib/settings.js";
import { resolveWorktree, type ResolveOptions } from "./_resolve.js";
import type { Settings } from "../lib/types.js";

export function runOpen(opts: ResolveOptions, flags: { ide?: Settings["ide"] }): void {
  const wt = resolveWorktree(opts);
  const settings = loadSettings();
  const ide = flags.ide ?? settings.ide;
  openInIde(wt.path, ide);
  console.log(`Opened ${wt.branch} in ${ide}`);
}
