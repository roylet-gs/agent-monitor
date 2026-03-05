import { deleteWorktree, deleteBranch, deleteRemoteBranch, remoteBranchExists } from "../../lib/git.js";
import { removeWorktree, getRepositoryById } from "../../lib/db.js";
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
