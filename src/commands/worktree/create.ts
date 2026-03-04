import { createWorktree, branchExists, getMainBranch, fetchBranch, remoteBranchExists } from "../../lib/git.js";
import { upsertWorktree } from "../../lib/db.js";
import { syncWorktrees } from "../../lib/sync.js";
import { resolveRepo } from "../../lib/resolve.js";
import { installGlobalHooks, isGlobalHooksInstalled } from "../../lib/hooks-installer.js";
import { outputJson } from "../../lib/output.js";

export async function worktreeCreate(
  branch: string,
  opts: { repo?: string; name?: string; base?: string; reuse?: boolean; json?: boolean }
): Promise<void> {
  const repo = resolveRepo(opts.repo);

  // Check if branch already exists
  const exists = await branchExists(repo.path, branch);
  if (exists && !opts.reuse) {
    console.error(`Branch "${branch}" already exists. Use --reuse to attach to the existing branch.`);
    process.exit(1);
  }

  // Determine base branch
  const baseBranch = opts.base ?? (await getMainBranch(repo.path));

  // Fetch the base branch to ensure we're up to date
  if (!opts.reuse) {
    await fetchBranch(repo.path, baseBranch);
  }

  // If reusing, fetch the remote branch
  if (opts.reuse) {
    const hasRemote = await remoteBranchExists(repo.path, branch);
    if (hasRemote) {
      await fetchBranch(repo.path, branch);
    }
  }

  // Create the worktree
  const worktreePath = await createWorktree(repo.path, branch, baseBranch, opts.reuse);

  // Sync DB
  await syncWorktrees(repo.id);

  // Ensure hooks are installed
  if (!isGlobalHooksInstalled()) {
    installGlobalHooks();
    console.log("Claude hooks installed automatically.");
  }

  if (opts.json) {
    outputJson({ branch, path: worktreePath, repo: repo.name });
  } else {
    console.log(`Created worktree: ${worktreePath}`);
    console.log(`Branch: ${branch} (based on ${baseBranch})`);
  }
}
