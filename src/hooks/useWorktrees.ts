import { useState, useEffect, useCallback } from "react";
import { getWorktrees, getAgentStatuses } from "../lib/db.js";
import { getGitStatus, getLastCommit } from "../lib/git.js";
import { log } from "../lib/logger.js";
import type { WorktreeWithStatus } from "../lib/types.js";

export function useWorktrees(
  repoId: string | null,
  pollingIntervalMs: number
): {
  worktrees: WorktreeWithStatus[];
  refresh: () => Promise<void>;
} {
  const [worktrees, setWorktrees] = useState<WorktreeWithStatus[]>([]);

  const refresh = useCallback(async () => {
    if (!repoId) {
      setWorktrees([]);
      return;
    }

    try {
      const dbWorktrees = getWorktrees(repoId);
      const statuses = getAgentStatuses(repoId);

      const enriched: WorktreeWithStatus[] = await Promise.all(
        dbWorktrees.map(async (wt) => {
          let git_status = null;
          let last_commit = null;
          try {
            [git_status, last_commit] = await Promise.all([
              getGitStatus(wt.path),
              getLastCommit(wt.path),
            ]);
          } catch (err) {
            log("warn", "useWorktrees", `Failed to get git info for ${wt.path}: ${err}`);
          }

          return {
            ...wt,
            agent_status: statuses.get(wt.id) ?? null,
            git_status,
            last_commit,
          };
        })
      );

      setWorktrees(enriched);
    } catch (err) {
      log("error", "useWorktrees", `Failed to refresh worktrees: ${err}`);
    }
  }, [repoId]);

  useEffect(() => {
    refresh();
    const timer = setInterval(refresh, pollingIntervalMs);
    return () => clearInterval(timer);
  }, [refresh, pollingIntervalMs]);

  return { worktrees, refresh };
}
