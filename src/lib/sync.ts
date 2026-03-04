import { listWorktrees, getRepoName } from "./git.js";
import { getWorktrees, upsertWorktree, removeWorktree, getRepositoryById } from "./db.js";
import { log } from "./logger.js";
import { basename } from "path";

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

  // Add new worktrees from git that aren't in DB
  for (const gw of gitWorktrees) {
    if (!dbBranches.has(gw.branch)) {
      const shortName = gw.branch.split("/").pop() ?? gw.branch;
      upsertWorktree(repoId, gw.path, gw.branch, shortName);
      log("info", "sync", `Added worktree ${gw.branch} to DB`);
    }
  }

  // Remove DB entries for worktrees that no longer exist in git
  for (const dw of dbWorktrees) {
    if (!gitBranches.has(dw.branch)) {
      removeWorktree(dw.id);
      log("info", "sync", `Removed worktree ${dw.branch} from DB (no longer in git)`);
    }
  }

  // Update paths for existing worktrees (in case they moved)
  for (const gw of gitWorktrees) {
    if (dbBranches.has(gw.branch)) {
      upsertWorktree(repoId, gw.path, gw.branch, gw.branch.split("/").pop() ?? gw.branch);
    }
  }

  log("info", "sync", `Synced worktrees for repo ${repo.name}: ${gitWorktrees.length} in git`);
}
