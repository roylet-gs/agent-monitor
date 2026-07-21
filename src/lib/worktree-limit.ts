import type { Settings } from "./types.js";
import { getWorktrees } from "./db.js";
import { log } from "./logger.js";

/**
 * Enforces the optional per-repo worktree cap.
 *
 * Counts dedicated worktrees for a repo (excluding the main checkout) and, when
 * the limit is enabled and reached, returns a user-facing block message.
 * Returns null when creation is allowed. Shared by the TUI (src/app.tsx) and the
 * CLI (src/commands/worktree/create.ts) so both enforce identically.
 */
export function checkWorktreeLimit(
  settings: Settings,
  repoId: string,
  repoName: string
): string | null {
  if (!settings.worktreeLimitEnabled) return null;

  const dedicated = getWorktrees(repoId).filter((w) => !w.is_main).length;
  if (dedicated < settings.maxWorktrees) return null;

  log(
    "info",
    "worktree-limit",
    `Blocked worktree creation for ${repoName}: ${dedicated}/${settings.maxWorktrees} dedicated worktrees`
  );
  return (
    `Worktree limit reached (${dedicated}/${settings.maxWorktrees}) for ${repoName}.\n` +
    `Complete or delete an existing worktree before creating a new one.\n` +
    `Adjust or disable the limit in Settings.`
  );
}
