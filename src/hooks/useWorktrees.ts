import { useState, useEffect, useCallback, useRef } from "react";
import { getWorktrees, getAgentStatuses, updateWorktreeCustomName, clearLinearNicknames } from "../lib/db.js";
import { getGitStatus, getLastCommit } from "../lib/git.js";
import { fetchAllPrInfo } from "../lib/github.js";
import { fetchLinearInfo, linearAttachmentToPrInfo } from "../lib/linear.js";
import { log } from "../lib/logger.js";
import { getTerminalPaths, getIdePaths } from "../lib/process.js";
import { realpathSync } from "fs";
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
  ghRefreshOnManual: boolean;
  linearRefreshOnManual: boolean;
  linearAutoNickname: boolean;
}

export function useWorktrees(config: WorktreeHookConfig): {
  groups: WorktreeGroup[];
  flatWorktrees: WorktreeWithStatus[];
  refresh: () => Promise<void>;
  lightRefresh: () => Promise<void>;
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
    ghRefreshOnManual,
    linearRefreshOnManual,
    linearAutoNickname,
  } = config;

  const [groups, setGroups] = useState<WorktreeGroup[]>([]);
  const [flatWorktrees, setFlatWorktrees] = useState<WorktreeWithStatus[]>([]);
  const prCacheRef = useRef<Map<string, PrInfo | null>>(new Map());
  const prNumberCacheRef = useRef<Map<string, number>>(new Map());
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
  const ghRefreshOnManualRef = useRef(ghRefreshOnManual);
  ghRefreshOnManualRef.current = ghRefreshOnManual;
  const linearRefreshOnManualRef = useRef(linearRefreshOnManual);
  linearRefreshOnManualRef.current = linearRefreshOnManual;
  const linearAutoNicknameRef = useRef(linearAutoNickname);
  linearAutoNicknameRef.current = linearAutoNickname;

  // Generation counter: stale refresh calls check this before setting state
  const genRef = useRef(0);

  // Fetch PR info for all branches, batched by repo, and update cache
  const refreshPrInfo = useCallback(async (repoGroups: Array<{ repoPath: string; repoId: string; branches: string[] }>) => {
    if (repoGroups.length === 0) return;
    await Promise.all(
      repoGroups.map(async ({ repoPath, repoId, branches }) => {
        if (branches.length === 0) return;
        // Build per-repo PR number cache from the shared ref
        const repoPrNumbers = new Map<string, number>();
        for (const branch of branches) {
          const num = prNumberCacheRef.current.get(`${repoId}:${branch}`);
          if (num != null) repoPrNumbers.set(branch, num);
        }
        try {
          // Build per-repo PR cache for smart skip logic
          const repoPrCache = new Map<string, PrInfo | null>();
          for (const branch of branches) {
            const cacheKey = `${repoId}:${branch}`;
            if (prCacheRef.current.has(cacheKey)) {
              repoPrCache.set(branch, prCacheRef.current.get(cacheKey)!);
            }
          }
          const prMap = await fetchAllPrInfo(repoPath, branches, repoPrNumbers, repoPrCache);
          for (const [branch, info] of prMap) {
            const cacheKey = `${repoId}:${branch}`;
            // Preserve stale cache when backoff returns null
            if (info !== null || !prCacheRef.current.has(cacheKey)) {
              prCacheRef.current.set(cacheKey, info);
            }
            // Cache PR number for cheaper subsequent fetches
            if (info?.number != null) {
              prNumberCacheRef.current.set(cacheKey, info.number);
            }
          }
        } catch (err) {
          log("warn", "useWorktrees", `Batch PR fetch failed for repo ${repoId}, keeping stale cache: ${err}`);
        }
      })
    );
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

  // Auto-set worktree nicknames from Linear ticket titles
  const autoSetLinearNicknames = useCallback(() => {
    if (!linearAutoNicknameRef.current || !linearEnabledRef.current) return;
    for (const repo of reposRef.current) {
      const dbWorktrees = getWorktrees(repo.id);
      for (const wt of dbWorktrees) {
        if (wt.custom_name) continue;
        const linearInfo = linearCacheRef.current.get(wt.branch);
        if (!linearInfo) continue;
        log("info", "useWorktrees", `Auto-setting nickname for ${wt.branch} from Linear: "${linearInfo.title}"`);
        updateWorktreeCustomName(wt.id, linearInfo.title, "linear");
      }
    }
  }, []);

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
        const repoGroups: Array<{ repoPath: string; repoId: string; branches: string[] }> = [];
        const allBranchNames: string[] = [];
        for (const repo of repos) {
          const dbWorktrees = getWorktrees(repo.id);
          const ghBranches: string[] = [];
          for (const wt of dbWorktrees) {
            allBranchNames.push(wt.branch);
            // Only fetch gh for branches without Linear PR data
            if (!linearCacheRef.current.get(wt.branch)?.prAttachment) {
              ghBranches.push(wt.branch);
            }
          }
          repoGroups.push({ repoPath: repo.path, repoId: repo.id, branches: ghBranches });
        }
        const shouldRefreshPr = shouldFetchPr && ghRefreshOnManualRef.current;
        const shouldRefreshLinear = shouldFetchLinear && linearRefreshOnManualRef.current;
        await Promise.all([
          shouldRefreshPr ? refreshPrInfo(repoGroups) : Promise.resolve(),
          shouldRefreshLinear ? refreshLinearInfo(allBranchNames) : Promise.resolve(),
        ]);
        autoSetLinearNicknames();
        // Bail if a newer refresh started while we were fetching
        if (myGen !== genRef.current) return;
      }

      const newGroups: WorktreeGroup[] = [];
      const allFlat: WorktreeWithStatus[] = [];

      // Single lsof/ps call for all worktrees
      const terminalPaths = getTerminalPaths();
      const idePaths = getIdePaths();

      for (const repo of repos) {
        const dbWorktrees = getWorktrees(repo.id);
        const statuses = getAgentStatuses(repo.id);

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

            let has_terminal = false;
            let open_ide: "cursor" | "vscode" | null = null;
            try {
              const realPath = realpathSync(wt.path);
              has_terminal = terminalPaths.has(realPath);
              open_ide = idePaths.get(realPath) ?? null;
            } catch {
              // path doesn't exist or can't be resolved
            }

            return {
              ...wt,
              agent_status: statuses.get(wt.id) ?? null,
              git_status,
              last_commit,
              has_terminal,
              open_ide,
              pr_info: (() => {
                const linearInfo = linearCacheRef.current.get(wt.branch);
                if (linearInfo?.prAttachment) return linearAttachmentToPrInfo(linearInfo.prAttachment);
                return prCacheRef.current.get(`${repo.id}:${wt.branch}`) ?? null;
              })(),
              linear_info: linearCacheRef.current.get(wt.branch) ?? null,
            };
          })
        );

        // Bail if a newer refresh started while we were enriching
        if (myGen !== genRef.current) return;

        enriched.sort((a, b) => {
          // Dedicated worktrees first, main worktree branches last
          if (a.is_main !== b.is_main) return a.is_main - b.is_main;
          return b.created_at.localeCompare(a.created_at);
        });

        const filtered = shouldHideMain
          ? enriched.filter((wt) => !(wt.is_main === 1 && (wt.branch === "main" || wt.branch === "master")))
          : enriched;

        if (filtered.length > 0 || repos.length === 1) {
          newGroups.push({ repo, worktrees: filtered });
        }
        allFlat.push(...filtered);
      }

      // Final staleness check before committing state
      if (myGen !== genRef.current) return;

      const fingerprint = JSON.stringify(allFlat.map(wt => ({
        id: wt.id, branch: wt.branch, custom_name: wt.custom_name, is_main: wt.is_main,
        status: wt.agent_status?.status,
        is_open: wt.agent_status?.is_open,
        summary: wt.agent_status?.transcript_summary,
        response: wt.agent_status?.last_response,
        ahead: wt.git_status?.ahead, behind: wt.git_status?.behind,
        dirty: wt.git_status?.dirty,
        commit_msg: wt.last_commit?.message, commit_time: wt.last_commit?.relative_time,
        has_terminal: wt.has_terminal, open_ide: wt.open_ide,
        pr: wt.pr_info?.number, pr_state: wt.pr_info?.state, checks: wt.pr_info?.checksStatus,
        active_check: wt.pr_info?.activeCheckUrl, checks_waiting: wt.pr_info?.checksWaiting,
        linear: wt.linear_info?.identifier, linear_state: wt.linear_info?.state?.type,
        linear_pr_url: wt.linear_info?.prAttachment?.url,
      })));
      if (fingerprint !== prevFingerprintRef.current) {
        prevFingerprintRef.current = fingerprint;
        setGroups(newGroups);
        setFlatWorktrees(allFlat);
      }
    } catch (err) {
      log("error", "useWorktrees", `Failed to refresh worktrees: ${err}`);
    }
  }, [refreshPrInfo, refreshLinearInfo, autoSetLinearNicknames]);

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
      const repoGroups: Array<{ repoPath: string; repoId: string; branches: string[] }> = [];
      for (const repo of reposRef.current) {
        const dbWorktrees = getWorktrees(repo.id);
        // Skip branches where Linear already provides PR data
        const branches = dbWorktrees
          .map((wt) => wt.branch)
          .filter((b) => !linearCacheRef.current.get(b)?.prAttachment);
        repoGroups.push({ repoPath: repo.path, repoId: repo.id, branches });
      }
      await refreshPrInfo(repoGroups);
      refresh(false);
    };

    doFetch();
    const timer = setInterval(doFetch, ghPollingIntervalMs);
    return () => clearInterval(timer);
  }, [repositories, ghPrStatus, ghPollingIntervalMs, refreshPrInfo, refresh]);

  // Clear Linear-sourced nicknames when the feature is turned off
  useEffect(() => {
    if (!linearEnabled || !linearAutoNickname) {
      clearLinearNicknames();
    }
  }, [linearEnabled, linearAutoNickname]);

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
      autoSetLinearNicknames();
      refresh(false);
    };

    doFetch();
    const timer = setInterval(doFetch, linearPollingIntervalMs);
    return () => clearInterval(timer);
  }, [repositories, linearEnabled, linearPollingIntervalMs, refreshLinearInfo, refresh]);

  // Exposed refresh always forces integrations fetch
  const forceRefresh = useCallback(() => refresh(true), [refresh]);

  // Light refresh: debounced to avoid excessive git status calls from rapid pub/sub events
  const lightRefreshTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const lightRefresh = useCallback(() => {
    if (lightRefreshTimerRef.current) return Promise.resolve();
    lightRefreshTimerRef.current = setTimeout(() => {
      lightRefreshTimerRef.current = null;
      refresh(false);
    }, 300);
    return Promise.resolve();
  }, [refresh]);

  // Cleanup debounce timer on unmount
  useEffect(() => {
    return () => {
      if (lightRefreshTimerRef.current) clearTimeout(lightRefreshTimerRef.current);
    };
  }, []);

  return { groups, flatWorktrees, refresh: forceRefresh, lightRefresh };
}
