import { openInIde } from "../../lib/ide-launcher.js";
import { loadSettings } from "../../lib/settings.js";
import { resolveWorktree, resolveRepo } from "../../lib/resolve.js";
import { ensureBranchForOpen } from "../../lib/git.js";

export async function worktreeOpen(target: string, opts: { repo?: string }): Promise<void> {
  const repo = resolveRepo(opts.repo);
  const worktree = resolveWorktree(target, repo.id);
  const settings = loadSettings();

  const result = await ensureBranchForOpen(worktree.path, worktree.branch, worktree.is_main === 1);
  if (!result.ready) {
    console.error(result.error ?? "Cannot open worktree");
    process.exit(1);
  }
  if (result.switched) {
    console.log(`Switched to branch ${worktree.branch}`);
  }

  openInIde(worktree.path, settings.ide);
  console.log(`Opened ${worktree.branch} in ${settings.ide}`);
}
