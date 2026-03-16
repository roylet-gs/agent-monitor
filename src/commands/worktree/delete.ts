import { existsSync } from "fs";
import { deleteWorktree, deleteBranch, deleteRemoteBranch, remoteBranchExists, checkoutBranch, getMainBranch } from "../../lib/git.js";
import { removeWorktree, getRepositoryById } from "../../lib/db.js";
import { syncWorktrees } from "../../lib/sync.js";
import { resolveWorktree, resolveRepo } from "../../lib/resolve.js";

export async function worktreeDelete(
  target: string,
  opts: { repo?: string; force?: boolean; deleteBranch?: boolean; deleteRemote?: boolean }
): Promise<void> {
  const repo = resolveRepo(opts.repo);
  const worktree = resolveWorktree(target, repo.id);
  const repoObj = getRepositoryById(worktree.repo_id);

  if (!repoObj) {
    console.error("Repository not found in DB.");
    process.exit(1);
  }

  const pathExists = existsSync(worktree.path);
  const isBranchOnly =
    (worktree.is_main === 1 && worktree.branch !== "main" && worktree.branch !== "master") ||
    (!pathExists && worktree.is_main !== 1);
  const isStale = !pathExists && worktree.is_main !== 1;

  if (isBranchOnly) {
    // For main worktree entries: checkout default branch first
    // For stale entries (path gone): skip checkout, go straight to cleanup
    if (!isStale) {
      const mainBranch = await getMainBranch(repoObj.path);
      try {
        await checkoutBranch(repoObj.path, mainBranch);
        console.log(`Switched to ${mainBranch}`);
      } catch (err) {
        console.error(`Failed to switch to ${mainBranch}. Commit or stash changes first.\n${err}`);
        process.exit(1);
      }
    }

    try {
      await deleteBranch(repoObj.path, worktree.branch, opts.force);
      console.log(`Deleted local branch: ${worktree.branch}`);
    } catch (err) {
      if (isStale) {
        console.log(`Local branch ${worktree.branch} already removed or not found.`);
      } else {
        console.error(`Failed to delete local branch: ${err}`);
        process.exit(1);
      }
    }

    // Delete remote branch if requested
    if (opts.deleteRemote) {
      const hasRemote = await remoteBranchExists(repoObj.path, worktree.branch);
      if (hasRemote) {
        try {
          await deleteRemoteBranch(repoObj.path, worktree.branch);
          console.log(`Deleted remote branch: origin/${worktree.branch}`);
        } catch (err) {
          console.error(`Failed to delete remote branch: ${err}`);
        }
      }
    }

    // Explicitly remove from DB for stale entries
    removeWorktree(worktree.id);
    await syncWorktrees(repoObj.id);
    console.log(`Deleted ${isStale ? "stale" : "branch-only"} entry: ${worktree.branch}`);
    return;
  }

  // Delete the git worktree
  try {
    await deleteWorktree(repoObj.path, worktree.path, opts.force);
  } catch (err) {
    if (opts.force) throw err;
    console.error(`Failed to delete worktree. Use --force to override.\n${err}`);
    process.exit(1);
  }

  // Delete local branch if requested
  if (opts.deleteBranch) {
    try {
      await deleteBranch(repoObj.path, worktree.branch, opts.force);
      console.log(`Deleted local branch: ${worktree.branch}`);
    } catch (err) {
      console.error(`Failed to delete local branch: ${err}`);
    }
  }

  // Delete remote branch if requested
  if (opts.deleteRemote) {
    const hasRemote = await remoteBranchExists(repoObj.path, worktree.branch);
    if (hasRemote) {
      try {
        await deleteRemoteBranch(repoObj.path, worktree.branch);
        console.log(`Deleted remote branch: origin/${worktree.branch}`);
      } catch (err) {
        console.error(`Failed to delete remote branch: ${err}`);
      }
    }
  }

  // Remove from DB
  removeWorktree(worktree.id);
  console.log(`Deleted worktree: ${worktree.path}`);
}
