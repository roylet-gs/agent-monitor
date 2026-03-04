import { useState, useEffect, useCallback, useRef } from "react";
import { getWorktrees, getAgentStatuses } from "../lib/db.js";
import { getGitStatus, getLastCommit } from "../lib/git.js";
import { fetchPrInfo } from "../lib/github.js";
import { log } from "../lib/logger.js";
import type { WorktreeWithStatus, PrInfo } from "../lib/types.js";

export function useWorktrees(
  repoId: string | null,
  pollingIntervalMs: number,
  ghPrStatus: boolean = true,
  ghPollingIntervalMs: number = 60000
): {
  worktrees: WorktreeWithStatus[];
  refresh: () => Promise<void>;
} {
  const [worktrees, setWorktrees] = useState<WorktreeWithStatus[]>([]);
  const prCacheRef = useRef<Map<string, PrInfo | null>>(new Map());

  // Fetch PR info for all branches and update cache
  const refreshPrInfo = useCallback(async (branches: Array<{ path: string; branch: string }>) => {
    if (!ghPrStatus || branches.length === 0) return;
    const entries = await Promise.all(
      branches.map(async ({ path, branch }) => {
        try {
          const info = await fetchPrInfo(path, branch);
          return [branch, info] as const;
        } catch {
          return [branch, prCacheRef.current.get(branch) ?? null] as const;
        }
      })
    );
    for (const [branch, info] of entries) {
      prCacheRef.current.set(branch, info);
    }
  }, [ghPrStatus]);

  const refresh = useCallback(async (forcePr = false) => {
    if (!repoId) {
      setWorktrees([]);
      return;
    }

    try {
      const dbWorktrees = getWorktrees(repoId);
      const statuses = getAgentStatuses(repoId);

      // On forced refresh, fetch PR info first
      if (forcePr && ghPrStatus) {
        await refreshPrInfo(dbWorktrees.map((wt) => ({ path: wt.path, branch: wt.branch })));
      }

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
            pr_info: prCacheRef.current.get(wt.branch) ?? null,
          };
        })
      );

      enriched.sort((a, b) => {
        const timeA = a.agent_status?.updated_at ?? "";
        const timeB = b.agent_status?.updated_at ?? "";
        return timeB.localeCompare(timeA);
      });

      setWorktrees(enriched);
    } catch (err) {
      log("error", "useWorktrees", `Failed to refresh worktrees: ${err}`);
    }
  }, [repoId, ghPrStatus, refreshPrInfo]);

  // Main polling loop (git status, agent status — no PR fetching)
  useEffect(() => {
    refresh();
    const timer = setInterval(() => refresh(false), pollingIntervalMs);
    return () => clearInterval(timer);
  }, [refresh, pollingIntervalMs]);

  // Separate PR polling loop at a slower interval
  useEffect(() => {
    if (!ghPrStatus || !repoId) return;

    // Initial PR fetch
    const doFetch = async () => {
      const dbWorktrees = getWorktrees(repoId);
      await refreshPrInfo(dbWorktrees.map((wt) => ({ path: wt.path, branch: wt.branch })));
      // Trigger a re-render with updated cache
      refresh(false);
    };

    doFetch();
    const timer = setInterval(doFetch, ghPollingIntervalMs);
    return () => clearInterval(timer);
  }, [repoId, ghPrStatus, ghPollingIntervalMs, refreshPrInfo, refresh]);

  // Exposed refresh always forces PR fetch
  const forceRefresh = useCallback(() => refresh(true), [refresh]);

  return { worktrees, refresh: forceRefresh };
}
