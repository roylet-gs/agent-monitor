import { createInterface } from "readline";
import { getRepositoryById, removeWorktree } from "../lib/db.js";
import { deleteWorktree, deleteBranch } from "../lib/git.js";
import { resolveWorktree, type ResolveOptions } from "./_resolve.js";

export interface DeleteFlags {
  deleteBranch?: boolean;
  force?: boolean;
  yes?: boolean;
}

export async function runDelete(opts: ResolveOptions, flags: DeleteFlags): Promise<void> {
  const wt = resolveWorktree(opts);
  const repo = getRepositoryById(wt.repo_id);
  if (!repo) {
    console.error("Error: repository not found for worktree");
    process.exit(1);
  }

  if (!flags.yes) {
    const confirmed = await confirm(
      `Delete worktree "${wt.branch}" at ${wt.path}?${flags.deleteBranch ? " (branch will also be deleted)" : ""} [y/N] `
    );
    if (!confirmed) {
      console.log("Cancelled.");
      return;
    }
  }

  // Remove the git worktree
  try {
    await deleteWorktree(repo.path, wt.path, flags.force);
    console.log(`Deleted worktree at ${wt.path}`);
  } catch (err) {
    console.error(`Error deleting worktree: ${err}`);
    if (!flags.force) {
      console.error("Tip: use --force to force removal");
    }
    process.exit(1);
  }

  // Remove from DB
  removeWorktree(wt.id);

  // Delete the branch if requested
  if (flags.deleteBranch) {
    try {
      await deleteBranch(repo.path, wt.branch, flags.force);
      console.log(`Deleted branch: ${wt.branch}`);
    } catch (err) {
      console.error(`Warning: could not delete branch "${wt.branch}": ${err}`);
    }
  }
}

function confirm(prompt: string): Promise<boolean> {
  const rl = createInterface({ input: process.stdin, output: process.stdout });
  return new Promise((res) => {
    rl.question(prompt, (answer) => {
      rl.close();
      res(answer.toLowerCase() === "y" || answer.toLowerCase() === "yes");
    });
  });
}
