import { useState, useEffect, useCallback, useRef } from "react";
import { getWorktrees, getAgentStatuses } from "../lib/db.js";
import { getGitStatus, getLastCommit } from "../lib/git.js";
import { fetchPrInfo } from "../lib/github.js";
import { fetchLinearInfo } from "../lib/linear.js";
import { log } from "../lib/logger.js";
import type { WorktreeWithStatus, PrInfo, LinearInfo } from "../lib/types.js";

export interface WorktreeHookConfig {
  repoId: string | null;
  pollingIntervalMs: number;
  ghPollingIntervalMs: number;
  linearPollingIntervalMs: number;
  ghPrStatus: boolean;
  linearEnabled: boolean;
  linearApiKey: string;
}

export function useWorktrees(config: WorktreeHookConfig): {
  worktrees: WorktreeWithStatus[];
  refresh: () => Promise<void>;
} {
  const {
    repoId,
    pollingIntervalMs,
    ghPollingIntervalMs,
    linearPollingIntervalMs,
    ghPrStatus,
    linearEnabled,
    linearApiKey,
  } = config;

  const [worktrees, setWorktrees] = useState<WorktreeWithStatus[]>([]);
  const prCacheRef = useRef<Map<string, PrInfo | null>>(new Map());
  const linearCacheRef = useRef<Map<string, LinearInfo | null>>(new Map());

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

  // Fetch Linear info for all branches and update cache
  const refreshLinearInfo = useCallback(async (branches: string[]) => {
    if (!linearEnabled || !linearApiKey || branches.length === 0) return;

    const entries = await Promise.all(
      branches.map(async (branch) => {
        try {
          const info = await fetchLinearInfo(branch, linearApiKey);
          return [branch, info] as const;
        } catch {
          return [branch, linearCacheRef.current.get(branch) ?? null] as const;
        }
      })
    );
    for (const [branch, info] of entries) {
      linearCacheRef.current.set(branch, info);
    }
  }, [linearEnabled, linearApiKey]);

  const refresh = useCallback(async (forceIntegrations = false) => {
    if (!repoId) {
      setWorktrees([]);
      return;
    }

    try {
      const dbWorktrees = getWorktrees(repoId);
      const statuses = getAgentStatuses(repoId);

      // On forced refresh, fetch integrations first
      if (forceIntegrations) {
        const branchesForPr = dbWorktrees.map((wt) => ({ path: wt.path, branch: wt.branch }));
        const branchNames = dbWorktrees.map((wt) => wt.branch);
        await Promise.all([
          ghPrStatus ? refreshPrInfo(branchesForPr) : Promise.resolve(),
          linearEnabled ? refreshLinearInfo(branchNames) : Promise.resolve(),
        ]);
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
            linear_info: linearCacheRef.current.get(wt.branch) ?? null,
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
  }, [repoId, ghPrStatus, linearEnabled, refreshPrInfo, refreshLinearInfo]);

  // Main polling loop (git status, agent status — no integrations)
  useEffect(() => {
    refresh();
    const timer = setInterval(() => refresh(false), pollingIntervalMs);
    return () => clearInterval(timer);
  }, [refresh, pollingIntervalMs]);

  // GitHub PR polling loop
  useEffect(() => {
    if (!ghPrStatus || !repoId) return;

    const doFetch = async () => {
      const dbWorktrees = getWorktrees(repoId);
      const branchesForPr = dbWorktrees.map((wt) => ({ path: wt.path, branch: wt.branch }));
      await refreshPrInfo(branchesForPr);
      refresh(false);
    };

    doFetch();
    const timer = setInterval(doFetch, ghPollingIntervalMs);
    return () => clearInterval(timer);
  }, [repoId, ghPrStatus, ghPollingIntervalMs, refreshPrInfo, refresh]);

  // Linear polling loop
  useEffect(() => {
    if (!linearEnabled || !repoId) return;

    const doFetch = async () => {
      const dbWorktrees = getWorktrees(repoId);
      const branchNames = dbWorktrees.map((wt) => wt.branch);
      await refreshLinearInfo(branchNames);
      refresh(false);
    };

    doFetch();
    const timer = setInterval(doFetch, linearPollingIntervalMs);
    return () => clearInterval(timer);
  }, [repoId, linearEnabled, linearPollingIntervalMs, refreshLinearInfo, refresh]);

  // Exposed refresh always forces integrations fetch
  const forceRefresh = useCallback(() => refresh(true), [refresh]);

  return { worktrees, refresh: forceRefresh };
}
