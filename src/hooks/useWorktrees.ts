import { useState, useEffect, useCallback, useRef } from "react";
import { getWorktrees, getAgentStatuses, updateWorktreeCustomName, clearLinearNicknames } from "../lib/db.js";
import { getGitStatus, getLastCommit } from "../lib/git.js";
import {
  cachedLinearInfo,
  clearCachedLinear,
  getIntegrationHealth,
  hydrateIntegrationCache,
  integrationFieldsFor,
  refreshLinearCache,
  refreshPrCache,
  type RepoBranches,
} from "../lib/integration-cache.js";
import { buildGroups, applyWorktreeFilters, type RepoWorktrees } from "../lib/grouping.js";
import { log } from "../lib/logger.js";
import { syncWorktrees } from "../lib/sync.js";
import { getTerminalPaths, getIdePaths, getWorktreeProcesses, processesForWorktree } from "../lib/process.js";
import { realpathSync } from "fs";
import type { WorktreeWithStatus, WorktreeGroup, IntegrationHealth, Repository, WorktreeSortCriterion, RunningProcess } from "../lib/types.js";

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
  worktreeSort: WorktreeSortCriterion[];
  hideMergedClosedPrs: boolean;
  hideIdleDoneAgents: boolean;
  hideWithoutLinearTicket: boolean;
  showRunningProcesses: boolean;
  runningProcessFilter: string;
}

/** All repos with their worktree branches — the unit both integration fetches take. */
function buildRepoGroups(repos: Repository[]): RepoBranches[] {
  return repos.map((repo) => ({
    repoPath: repo.path,
    repoId: repo.id,
    branches: getWorktrees(repo.id).map((wt) => wt.branch),
  }));
}

export function useWorktrees(config: WorktreeHookConfig): {
  groups: WorktreeGroup[];
  flatWorktrees: WorktreeWithStatus[];
  integrationHealth: IntegrationHealth;
  refresh: () => Promise<void>;
  lightRefresh: () => Promise<void>;
  quickRefresh: () => Promise<void>;
  refreshIntegrations: (onStatus?: (status: string | null) => void) => Promise<void>;
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
    worktreeSort,
    hideMergedClosedPrs,
    hideIdleDoneAgents,
    hideWithoutLinearTicket,
    showRunningProcesses,
    runningProcessFilter,
  } = config;

  const [data, setData] = useState<{
    groups: WorktreeGroup[];
    flatWorktrees: WorktreeWithStatus[];
    integrationHealth: IntegrationHealth;
  }>({ groups: [], flatWorktrees: [], integrationHealth: getIntegrationHealth() });
  const prevFingerprintRef = useRef("");

  // Load persisted PR/Linear data before the first render pass. This has to be
  // synchronous — the first refresh() fires from the [repositories] effect, and a
  // useEffect would land too late, painting one empty frame (the very symptom the
  // cache exists to prevent).
  useState(() => {
    hydrateIntegrationCache();
    return null;
  });

  // Keep refs for values that refresh needs, so it always reads the latest
  const reposRef = useRef(repositories);
  reposRef.current = repositories;
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
  const linearApiKeyRef = useRef(linearApiKey);
  linearApiKeyRef.current = linearApiKey;
  const worktreeSortRef = useRef(worktreeSort);
  worktreeSortRef.current = worktreeSort;
  const showRunningProcessesRef = useRef(showRunningProcesses);
  showRunningProcessesRef.current = showRunningProcesses;
  const runningProcessFilterRef = useRef(runningProcessFilter);
  runningProcessFilterRef.current = runningProcessFilter;
  const filtersRef = useRef({ hideMainBranch, hideMergedClosedPrs, hideIdleDoneAgents, hideWithoutLinearTicket });
  filtersRef.current = { hideMainBranch, hideMergedClosedPrs, hideIdleDoneAgents, hideWithoutLinearTicket };

  // Generation counter: stale refresh calls check this before setting state
  const genRef = useRef(0);

  // Store integration functions in refs so refresh can have [] deps
  const refreshPrInfoRef = useRef<(repoGroups: RepoBranches[], force?: boolean) => Promise<void>>(async () => {});
  const refreshLinearInfoRef = useRef<(repoGroups: RepoBranches[]) => Promise<void>>(async () => {});
  const autoSetLinearNicknamesRef = useRef<() => void>(() => {});

  // Caching, stale-preservation and persistence all live in integration-cache.ts,
  // shared with the daemon so the two can't diverge.
  const refreshPrInfo = useCallback(async (repoGroups: RepoBranches[], force = false) => {
    await refreshPrCache(repoGroups, { force });
  }, []);

  const refreshLinearInfo = useCallback(async (repoGroups: RepoBranches[]) => {
    await refreshLinearCache(repoGroups, linearApiKeyRef.current);
  }, []);

  // Auto-set worktree nicknames from Linear ticket titles
  const autoSetLinearNicknames = useCallback(() => {
    if (!linearAutoNicknameRef.current || !linearEnabledRef.current) return;
    for (const repo of reposRef.current) {
      const dbWorktrees = getWorktrees(repo.id);
      for (const wt of dbWorktrees) {
        if (wt.custom_name) continue;
        const linearInfo = cachedLinearInfo(repo.id, wt.branch);
        if (!linearInfo) continue;
        log("info", "useWorktrees", `Auto-setting nickname for ${wt.branch} from Linear: "${linearInfo.title}"`);
        updateWorktreeCustomName(wt.id, linearInfo.title, "linear");
      }
    }
  }, []);

  // Keep refs in sync so refresh (with [] deps) always calls the latest versions
  refreshPrInfoRef.current = refreshPrInfo;
  refreshLinearInfoRef.current = refreshLinearInfo;
  autoSetLinearNicknamesRef.current = autoSetLinearNicknames;

  // Single stable refresh function that reads latest values from refs
  const refresh = useCallback(async (forceIntegrations = false) => {
    const myGen = ++genRef.current;
    const repos = reposRef.current;
    const shouldFetchPr = ghPrStatusRef.current;
    const shouldFetchLinear = linearEnabledRef.current;

    if (repos.length === 0) {
      setData((prev) => prev.groups.length === 0 && prev.flatWorktrees.length === 0 ? prev : { ...prev, groups: [], flatWorktrees: [] });
      return;
    }

    try {
      // Collect all branches for integration fetches if forced
      if (forceIntegrations) {
        const repoGroups = buildRepoGroups(repos);
        const shouldRefreshPr = shouldFetchPr && ghRefreshOnManualRef.current;
        const shouldRefreshLinear = shouldFetchLinear && linearRefreshOnManualRef.current;
        await Promise.all([
          shouldRefreshPr ? refreshPrInfoRef.current(repoGroups, true) : Promise.resolve(),
          shouldRefreshLinear ? refreshLinearInfoRef.current(repoGroups) : Promise.resolve(),
        ]);
        autoSetLinearNicknamesRef.current();
        // Bail if a newer refresh started while we were fetching
        if (myGen !== genRef.current) return;
      }

      const perRepo: RepoWorktrees[] = [];

      // Single lsof/ps call for all worktrees
      const terminalPaths = getTerminalPaths();
      const idePaths = getIdePaths();

      // Optional running sub-process scan + all worktree real paths (for
      // subtree attribution). Skipped entirely when the feature is off.
      const showProcs = showRunningProcessesRef.current;
      const procMap = showProcs ? getWorktreeProcesses(runningProcessFilterRef.current) : new Map<string, RunningProcess[]>();
      const allWorktreeRealPaths: string[] = [];
      if (showProcs) {
        for (const repo of repos) {
          for (const wt of getWorktrees(repo.id)) {
            try { allWorktreeRealPaths.push(realpathSync(wt.path)); } catch { /* unresolved */ }
          }
        }
      }

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
            let running_processes: RunningProcess[] = [];
            try {
              const realPath = realpathSync(wt.path);
              has_terminal = terminalPaths.has(realPath);
              open_ide = idePaths.get(realPath) ?? null;
              if (showProcs) running_processes = processesForWorktree(procMap, realPath, allWorktreeRealPaths);
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
              running_processes,
              ...integrationFieldsFor(repo.id, wt.branch),
            };
          })
        );

        // Bail if a newer refresh started while we were enriching
        if (myGen !== genRef.current) return;

        const filtered = applyWorktreeFilters(enriched, filtersRef.current);

        perRepo.push({ repo, worktrees: filtered });
      }

      // Sorting (incl. Linear ticket clustering) and project-major bucketing
      // both live in buildGroups, shared with the daemon.
      const { groups: newGroups, flatWorktrees: allFlat } = buildGroups(perRepo, worktreeSortRef.current);

      // Final staleness check before committing state
      if (myGen !== genRef.current) return;

      const integrationHealth = getIntegrationHealth();
      const fingerprint = JSON.stringify(allFlat.map(wt => ({
        id: wt.id, branch: wt.branch, custom_name: wt.custom_name, is_main: wt.is_main,
        status: wt.agent_status?.status,
        is_open: wt.agent_status?.is_open,
        session_id: wt.agent_status?.session_id,
        updated_at: wt.agent_status?.updated_at,
        summary: wt.agent_status?.transcript_summary,
        response: wt.agent_status?.last_response,
        ahead: wt.git_status?.ahead, behind: wt.git_status?.behind,
        dirty: wt.git_status?.dirty,
        commit_msg: wt.last_commit?.message, commit_time: wt.last_commit?.relative_time,
        has_terminal: wt.has_terminal, open_ide: wt.open_ide,
        procs: wt.running_processes.map(p => p.pid).join(","),
        pr: wt.pr_info?.number, pr_state: wt.pr_info?.state, checks: wt.pr_info?.checksStatus,
        active_check: wt.pr_info?.activeCheckUrl, checks_waiting: wt.pr_info?.checksWaiting,
        linear: wt.linear_info?.identifier, linear_state: wt.linear_info?.state?.type,
        linear_pr_url: wt.linear_info?.prAttachment?.url,
        linear_project: wt.linear_info?.project?.id,
        linear_project_name: wt.linear_info?.project?.name,
      })))
        // Integration health drives the "showing cached data" hint in the action
        // bar; without it here the flag flips but the render is skipped.
        + `|gh:${integrationHealth.githubFailing ? 1 : 0}|ln:${integrationHealth.linearFailing ? 1 : 0}`;
      if (fingerprint !== prevFingerprintRef.current) {
        prevFingerprintRef.current = fingerprint;
        setData({ groups: newGroups, flatWorktrees: allFlat, integrationHealth });
      }
    } catch (err) {
      log("error", "useWorktrees", `Failed to refresh worktrees: ${err}`);
    }
  }, []);

  // Re-fetch immediately when repositories change
  useEffect(() => {
    refresh();
  }, [repositories]);

  // Main polling loop
  useEffect(() => {
    const timer = setInterval(() => refresh(false), pollingIntervalMs);
    return () => clearInterval(timer);
  }, [pollingIntervalMs]);

  // Re-scan immediately when the running-process toggle or filter changes so
  // the list reflects the new criteria without waiting for the next poll.
  useEffect(() => {
    refresh(false);
  }, [showRunningProcesses, runningProcessFilter]);

  // GitHub PR polling loop
  useEffect(() => {
    if (!ghPrStatus || repositories.length === 0) return;

    const doFetch = async () => {
      await refreshPrInfoRef.current(buildRepoGroups(reposRef.current));
      refresh(false);
    };

    doFetch();
    const timer = setInterval(doFetch, ghPollingIntervalMs);
    return () => clearInterval(timer);
  }, [repositories, ghPrStatus, ghPollingIntervalMs]);

  // Periodic worktree sync — discovers worktrees created outside of am
  useEffect(() => {
    if (repositories.length === 0) return;
    const syncIntervalMs = pollingIntervalMs * 2;
    const doSync = async () => {
      try {
        for (const repo of reposRef.current) {
          await syncWorktrees(repo.id);
        }
        refresh(false);
      } catch (err) {
        log("warn", "useWorktrees", `Periodic sync failed: ${err}`);
      }
    };
    doSync();
    const timer = setInterval(doSync, syncIntervalMs);
    return () => clearInterval(timer);
  }, [repositories, pollingIntervalMs]);

  // Clear Linear-sourced nicknames when the feature is turned off
  useEffect(() => {
    if (!linearEnabled || !linearAutoNickname) {
      clearLinearNicknames();
    }
  }, [linearEnabled, linearAutoNickname]);

  // Drop cached tickets when Linear is switched off or the key changes, so the
  // list stops grouping by data the user no longer wants (or can no longer verify).
  useEffect(() => {
    if (!linearEnabled) clearCachedLinear();
  }, [linearEnabled, linearApiKey]);

  // Linear polling loop
  useEffect(() => {
    if (!linearEnabled || repositories.length === 0) return;

    const doFetch = async () => {
      await refreshLinearInfoRef.current(buildRepoGroups(reposRef.current));
      autoSetLinearNicknamesRef.current();
      refresh(false);
    };

    doFetch();
    const timer = setInterval(doFetch, linearPollingIntervalMs);
    return () => clearInterval(timer);
  }, [repositories, linearEnabled, linearPollingIntervalMs]);

  // Exposed refresh always forces integrations fetch
  const forceRefresh = useCallback(() => refresh(true), []);

  // Quick refresh: immediate refresh using cached integration data (no debounce)
  const quickRefresh = useCallback(() => refresh(false), []);

  // Refresh only integrations (GitHub PR + Linear), then re-enrich from updated caches.
  // Optional onStatus callback reports which sources are still loading.
  const refreshIntegrations = useCallback(async (onStatus?: (status: string | null) => void) => {
    const repos = reposRef.current;
    const shouldFetchPr = ghPrStatusRef.current && ghRefreshOnManualRef.current;
    const shouldFetchLinear = linearEnabledRef.current && linearRefreshOnManualRef.current;
    if (!shouldFetchPr && !shouldFetchLinear) return;

    const repoGroups = buildRepoGroups(repos);

    // Track which sources are still pending for status reporting
    const pending = new Set<string>();
    if (shouldFetchPr) pending.add("GitHub");
    if (shouldFetchLinear) pending.add("Linear");

    const reportStatus = () => {
      if (!onStatus) return;
      if (pending.size === 0) { onStatus(null); return; }
      onStatus(`Syncing ${[...pending].join(", ")}…`);
    };

    reportStatus();

    await Promise.all([
      shouldFetchPr
        ? refreshPrInfoRef.current(repoGroups, true).then(() => { pending.delete("GitHub"); reportStatus(); })
        : Promise.resolve(),
      shouldFetchLinear
        ? refreshLinearInfoRef.current(repoGroups).then(() => { pending.delete("Linear"); reportStatus(); })
        : Promise.resolve(),
    ]);
    autoSetLinearNicknamesRef.current();

    // Re-enrich worktrees with fresh integration data
    await refresh(false);
  }, []);

  // Light refresh: debounced to avoid excessive git status calls from rapid pub/sub events
  const lightRefreshTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const lightRefresh = useCallback(() => {
    if (lightRefreshTimerRef.current) return Promise.resolve();
    lightRefreshTimerRef.current = setTimeout(() => {
      lightRefreshTimerRef.current = null;
      refresh(false);
    }, 300);
    return Promise.resolve();
  }, []);

  // Cleanup debounce timer on unmount
  useEffect(() => {
    return () => {
      if (lightRefreshTimerRef.current) clearTimeout(lightRefreshTimerRef.current);
    };
  }, []);

  return { groups: data.groups, flatWorktrees: data.flatWorktrees, integrationHealth: data.integrationHealth, refresh: forceRefresh, lightRefresh, quickRefresh, refreshIntegrations };
}
