import { listWorktrees, getRepoName } from "./git.js";
import { getWorktrees, upsertWorktree, removeWorktree, getRepositoryById } from "./db.js";
import { log } from "./logger.js";
import { existsSync } from "fs";
import { basename, join } from "path";

export async function syncWorktrees(repoId: string): Promise<void> {
  const repo = getRepositoryById(repoId);
  if (!repo) {
    log("warn", "sync", `Repository ${repoId} not found in DB`);
    return;
  }

  const gitWorktrees = await listWorktrees(repo.path);
  const dbWorktrees = getWorktrees(repoId);

  // Build set of git worktree branches for quick lookup
  const gitBranches = new Set(gitWorktrees.map((w) => w.branch));
  const dbBranches = new Set(dbWorktrees.map((w) => w.branch));
  const gitPathToBranch = new Map(gitWorktrees.map((w) => [w.path, w.branch]));

  // Remove DB entries for worktrees that no longer exist in git (before adding new ones)
  for (const dw of dbWorktrees) {
    if (!gitBranches.has(dw.branch)) {
      // Detect branch rename: same path in git but with a different branch
      const gitBranchAtPath = gitPathToBranch.get(dw.path);
      if (gitBranchAtPath && gitBranchAtPath !== dw.branch) {
        removeWorktree(dw.id);
        log("info", "sync", `Removed worktree ${dw.branch} from DB (branch renamed to ${gitBranchAtPath})`);
        continue;
      }
      // Safety net: don't delete if the worktree path still exists on disk
      // (likely detached/rebasing and recoverDetachedBranch didn't catch it)
      if (existsSync(join(dw.path, ".git"))) {
        log("debug", "sync", `Keeping worktree ${dw.branch} (path exists, likely detached/rebasing)`);
        continue;
      }
      removeWorktree(dw.id);
      log("info", "sync", `Removed worktree ${dw.branch} from DB (no longer in git)`);
    }
  }

  // Add new worktrees from git that aren't in DB
  for (const gw of gitWorktrees) {
    if (!dbBranches.has(gw.branch)) {
      const shortName = gw.branch.split("/").pop() ?? gw.branch;
      upsertWorktree(repoId, gw.path, gw.branch, shortName, gw.isMain);
      log("info", "sync", `Added worktree ${gw.branch} to DB${gw.isMain ? " (main)" : ""}`);
    }
  }

  // Update paths for existing worktrees (in case they moved)
  for (const gw of gitWorktrees) {
    if (dbBranches.has(gw.branch)) {
      upsertWorktree(repoId, gw.path, gw.branch, gw.branch.split("/").pop() ?? gw.branch, gw.isMain);
    }
  }

  log("info", "sync", `Synced worktrees for repo ${repo.name}: ${gitWorktrees.length} in git`);
}
