import { useState, useEffect, useCallback, useRef } from "react";
import { getWorktrees, getAgentStatuses } from "../lib/db.js";
import { fetchPrInfo } from "../lib/github.js";
import { fetchLinearInfo } from "../lib/linear.js";
import { enrichWorktree } from "../lib/enrich.js";
import { log } from "../lib/logger.js";
import type { WorktreeWithStatus, WorktreeGroup, PrInfo, LinearInfo, Repository } from "../lib/types.js";

export interface WorktreeHookConfig {
  repositories: Repository[];
  pollingIntervalMs: number;
  ghPollingIntervalMs: number;
  linearPollingIntervalMs: number;
  ghPrStatus: boolean;
  linearEnabled: boolean;
  linearApiKey: string;
  hideMainBranch: boolean;
}

export function useWorktrees(config: WorktreeHookConfig): {
  groups: WorktreeGroup[];
  flatWorktrees: WorktreeWithStatus[];
  refresh: () => Promise<void>;
} {
  const {
    repositories,
    pollingIntervalMs,
    ghPollingIntervalMs,
    linearPollingIntervalMs,
    ghPrStatus,
    linearEnabled,
    linearApiKey,
    hideMainBranch,
  } = config;

  const [groups, setGroups] = useState<WorktreeGroup[]>([]);
  const [flatWorktrees, setFlatWorktrees] = useState<WorktreeWithStatus[]>([]);
  const prCacheRef = useRef<Map<string, PrInfo | null>>(new Map());
  const linearCacheRef = useRef<Map<string, LinearInfo | null>>(new Map());
  const prevFingerprintRef = useRef("");

  // Keep refs for values that refresh needs, so it always reads the latest
  const reposRef = useRef(repositories);
  reposRef.current = repositories;
  const hideMainRef = useRef(hideMainBranch);
  hideMainRef.current = hideMainBranch;
  const ghPrStatusRef = useRef(ghPrStatus);
  ghPrStatusRef.current = ghPrStatus;
  const linearEnabledRef = useRef(linearEnabled);
  linearEnabledRef.current = linearEnabled;

  // Generation counter: stale refresh calls check this before setting state
  const genRef = useRef(0);

  // Fetch PR info for all branches and update cache
  const refreshPrInfo = useCallback(async (branches: Array<{ path: string; branch: string }>) => {
    if (branches.length === 0) return;
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
  }, []);

  const refreshLinearInfo = useCallback(async (branches: string[]) => {
    if (branches.length === 0) return;
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
  }, [linearApiKey]);

  // Single stable refresh function that reads latest values from refs
  const refresh = useCallback(async (forceIntegrations = false) => {
    const myGen = ++genRef.current;
    const repos = reposRef.current;
    const shouldHideMain = hideMainRef.current;
    const shouldFetchPr = ghPrStatusRef.current;
    const shouldFetchLinear = linearEnabledRef.current;

    if (repos.length === 0) {
      setGroups([]);
      setFlatWorktrees([]);
      return;
    }

    try {
      // Collect all branches for integration fetches if forced
      if (forceIntegrations) {
        const allBranchesForPr: Array<{ path: string; branch: string }> = [];
        const allBranchNames: string[] = [];
        for (const repo of repos) {
          const dbWorktrees = getWorktrees(repo.id);
          for (const wt of dbWorktrees) {
            allBranchesForPr.push({ path: wt.path, branch: wt.branch });
            allBranchNames.push(wt.branch);
          }
        }
        await Promise.all([
          shouldFetchPr ? refreshPrInfo(allBranchesForPr) : Promise.resolve(),
          shouldFetchLinear ? refreshLinearInfo(allBranchNames) : Promise.resolve(),
        ]);
        // Bail if a newer refresh started while we were fetching
        if (myGen !== genRef.current) return;
      }

      const newGroups: WorktreeGroup[] = [];
      const allFlat: WorktreeWithStatus[] = [];

      for (const repo of repos) {
        const dbWorktrees = getWorktrees(repo.id);
        const statuses = getAgentStatuses(repo.id);

        const enriched: WorktreeWithStatus[] = await Promise.all(
          dbWorktrees.map((wt) =>
            enrichWorktree(wt, statuses, repo.path, {
              prCache: prCacheRef.current,
              linearCache: linearCacheRef.current,
            })
          )
        );

        // Bail if a newer refresh started while we were enriching
        if (myGen !== genRef.current) return;

        enriched.sort((a, b) => b.created_at.localeCompare(a.created_at));

        const filtered = shouldHideMain
          ? enriched.filter((wt) => wt.branch !== "main" && wt.branch !== "master")
          : enriched;

        if (filtered.length > 0 || repos.length === 1) {
          newGroups.push({ repo, worktrees: filtered });
        }
        allFlat.push(...filtered);
      }

      // Final staleness check before committing state
      if (myGen !== genRef.current) return;

      const fingerprint = JSON.stringify(allFlat.map(wt => ({
        id: wt.id, branch: wt.branch, custom_name: wt.custom_name,
        status: wt.agent_status?.status,
        summary: wt.agent_status?.transcript_summary,
        response: wt.agent_status?.last_response,
        ahead: wt.git_status?.ahead, behind: wt.git_status?.behind,
        dirty: wt.git_status?.dirty,
        commit_msg: wt.last_commit?.message, commit_time: wt.last_commit?.relative_time,
        pr: wt.pr_info?.number, pr_state: wt.pr_info?.state, checks: wt.pr_info?.checksStatus,
        linear: wt.linear_info?.identifier, linear_state: wt.linear_info?.state?.type,
      })));
      if (fingerprint !== prevFingerprintRef.current) {
        prevFingerprintRef.current = fingerprint;
        setGroups(newGroups);
        setFlatWorktrees(allFlat);
      }
    } catch (err) {
      log("error", "useWorktrees", `Failed to refresh worktrees: ${err}`);
    }
  }, [refreshPrInfo, refreshLinearInfo]);

  // Re-fetch immediately when repositories change
  useEffect(() => {
    refresh();
  }, [repositories]);

  // Main polling loop
  useEffect(() => {
    const timer = setInterval(() => refresh(false), pollingIntervalMs);
    return () => clearInterval(timer);
  }, [refresh, pollingIntervalMs]);

  // GitHub PR polling loop
  useEffect(() => {
    if (!ghPrStatus || repositories.length === 0) return;

    const doFetch = async () => {
      const allBranches: Array<{ path: string; branch: string }> = [];
      for (const repo of reposRef.current) {
        const dbWorktrees = getWorktrees(repo.id);
        for (const wt of dbWorktrees) {
          allBranches.push({ path: wt.path, branch: wt.branch });
        }
      }
      await refreshPrInfo(allBranches);
      refresh(false);
    };

    doFetch();
    const timer = setInterval(doFetch, ghPollingIntervalMs);
    return () => clearInterval(timer);
  }, [repositories, ghPrStatus, ghPollingIntervalMs, refreshPrInfo, refresh]);

  // Linear polling loop
  useEffect(() => {
    if (!linearEnabled || repositories.length === 0) return;

    const doFetch = async () => {
      const allBranches: string[] = [];
      for (const repo of reposRef.current) {
        const dbWorktrees = getWorktrees(repo.id);
        for (const wt of dbWorktrees) {
          allBranches.push(wt.branch);
        }
      }
      await refreshLinearInfo(allBranches);
      refresh(false);
    };

    doFetch();
    const timer = setInterval(doFetch, linearPollingIntervalMs);
    return () => clearInterval(timer);
  }, [repositories, linearEnabled, linearPollingIntervalMs, refreshLinearInfo, refresh]);

  // Exposed refresh always forces integrations fetch
  const forceRefresh = useCallback(() => refresh(true), [refresh]);

  return { groups, flatWorktrees, refresh: forceRefresh };
}
