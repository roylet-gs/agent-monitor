import { openInIde } from "../../lib/ide-launcher.js";
import { loadSettings } from "../../lib/settings.js";
import { resolveWorktree, resolveRepo } from "../../lib/resolve.js";

export function worktreeOpen(target: string, opts: { repo?: string }): void {
  const repo = resolveRepo(opts.repo);
  const worktree = resolveWorktree(target, repo.id);
  const settings = loadSettings();
  openInIde(worktree.path, settings.ide);
  console.log(`Opened ${worktree.branch} in ${settings.ide}`);
}
