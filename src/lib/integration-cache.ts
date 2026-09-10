import {
  loadIntegrationCache,
  pruneIntegrationCache,
  setIntegrationCacheEntry,
  type IntegrationCacheKind,
} from "./db.js";
import { clearPrBackoff, fetchAllPrResults } from "./github.js";
import {
  fetchLinearResult,
  linearAttachmentMatchesBranch,
  linearAttachmentToPrInfo,
} from "./linear.js";
import { log } from "./logger.js";
import type { FetchResult, IntegrationHealth, LinearInfo, PrInfo } from "./types.js";

/**
 * Shared GitHub PR / Linear ticket cache, used by both the TUI hook
 * (`src/hooks/useWorktrees.ts`) and the daemon (`src/lib/daemon.ts`) so the two can't
 * diverge — the same reasoning behind `src/lib/grouping.ts`.
 *
 * Two rules make the dashboard survive being offline:
 *  1. An entry is only ever replaced by a *successful* fetch. A failure leaves the
 *     previous value in place, so PR statuses and Linear ticket/project grouping
 *     don't vanish when the network does.
 *  2. Entries are mirrored into SQLite, so they also survive the TUI's relaunch loop
 *     (`src/cli.tsx` unmounts Ink to run startup scripts) and a daemon restart.
 *
 * Entries never expire on their own — they live until a successful fetch replaces
 * them, or until their worktree disappears and they're pruned.
 */

export interface RepoBranches {
  repoPath: string;
  repoId: string;
  branches: string[];
}

/** Cache keys are repo-scoped so a branch name common to two repos can't collide. */
function cacheKey(repoId: string, branch: string): string {
  return `${repoId}:${branch}`;
}

const prCache = new Map<string, PrInfo | null>();
const linearCache = new Map<string, LinearInfo | null>();
/** Known PR numbers, for cheaper `gh pr view <number>` lookups. Not persisted. */
const prNumberCache = new Map<string, number>();

let hydrated = false;
const health: IntegrationHealth = {
  githubFailing: false,
  linearFailing: false,
  lastGithubError: null,
  lastLinearError: null,
};

function hydrateKind<T>(kind: IntegrationCacheKind, target: Map<string, T | null>): number {
  const rows = loadIntegrationCache(kind);
  let restored = 0;
  for (const [key, row] of rows) {
    if (row.payload === null) {
      target.set(key, null);
      restored++;
      continue;
    }
    try {
      target.set(key, JSON.parse(row.payload) as T);
      restored++;
    } catch (err) {
      log("warn", "integration-cache", `Dropping unparseable ${kind} cache row ${key}: ${err}`);
    }
  }
  return restored;
}

/**
 * Load persisted PR/Linear payloads into memory. Safe to call more than once; only
 * the first call reads the DB, so a TUI remount doesn't re-read on every mount.
 */
export function hydrateIntegrationCache(): void {
  if (hydrated) return;
  hydrated = true;
  try {
    const prs = hydrateKind<PrInfo>("pr", prCache);
    const linears = hydrateKind<LinearInfo>("linear", linearCache);
    for (const [key, info] of prCache) {
      if (info?.number != null && info.state !== "MERGED" && info.state !== "CLOSED") {
        prNumberCache.set(key, info.number);
      }
    }
    log("info", "integration-cache", `Hydrated ${prs} PR and ${linears} Linear cache entries from disk`);
  } catch (err) {
    log("warn", "integration-cache", `Failed to hydrate integration cache: ${err}`);
  }
}

/** Write through to memory + SQLite, but only for a fetch that actually succeeded. */
function commit<T>(
  kind: IntegrationCacheKind,
  key: string,
  mem: Map<string, T | null>,
  result: FetchResult<T>
): boolean {
  if (!result.ok) return false;
  if (result.value === null && mem.get(key)) {
    log("info", "integration-cache", `Clearing cached ${kind} for ${key}: upstream reports none`);
  }
  mem.set(key, result.value);
  try {
    setIntegrationCacheEntry(kind, key, result.value ? JSON.stringify(result.value) : null);
  } catch (err) {
    log("warn", "integration-cache", `Failed to persist ${kind} cache entry ${key}: ${err}`);
  }
  return true;
}

function prune(kind: IntegrationCacheKind, validKeys: string[], mem: Map<string, unknown>): void {
  // A momentarily empty worktree list (e.g. before the first sync) must not be
  // read as "everything is gone" — that would throw away the whole cache.
  if (validKeys.length === 0) return;
  try {
    const removed = pruneIntegrationCache(kind, validKeys);
    if (removed > 0) {
      log("debug", "integration-cache", `Pruned ${removed} stale ${kind} cache rows`);
    }
  } catch (err) {
    log("warn", "integration-cache", `Failed to prune ${kind} cache: ${err}`);
  }
  const valid = new Set(validKeys);
  for (const key of [...mem.keys()]) {
    if (!valid.has(key)) mem.delete(key);
  }
}

/**
 * Refresh cached PR info for every branch in `repoGroups`. Branches whose fetch
 * failed keep whatever they already had.
 *
 * `repoGroups` is expected to cover every known repo — entries outside it are pruned
 * as no-longer-present. Both callers build it from the full repository list.
 *
 * Pass `force` for a refresh the user asked for, so it isn't skipped by backoff.
 */
export async function refreshPrCache(
  repoGroups: RepoBranches[],
  opts: { force?: boolean } = {}
): Promise<void> {
  if (repoGroups.length === 0) return;
  hydrateIntegrationCache();

  let anyFailure: string | null = null;
  const allKeys: string[] = [];

  await Promise.all(
    repoGroups.map(async ({ repoPath, repoId, branches }) => {
      for (const branch of branches) allKeys.push(cacheKey(repoId, branch));
      if (branches.length === 0) return;
      // A user-initiated refresh isn't subject to the backoff — see clearPrBackoff.
      if (opts.force) clearPrBackoff(repoPath);

      // github.ts works in bare branch names; translate to repo-scoped keys here.
      const repoPrNumbers = new Map<string, number>();
      const repoPrCache = new Map<string, PrInfo | null>();
      for (const branch of branches) {
        const key = cacheKey(repoId, branch);
        const num = prNumberCache.get(key);
        if (num != null) repoPrNumbers.set(branch, num);
        if (prCache.has(key)) repoPrCache.set(branch, prCache.get(key)!);
      }

      try {
        const results = await fetchAllPrResults(repoPath, branches, repoPrNumbers, repoPrCache);
        for (const [branch, result] of results) {
          const key = cacheKey(repoId, branch);
          if (!result.ok) {
            anyFailure = result.error;
            continue;
          }
          commit("pr", key, prCache, result);
          // Cache the PR number for cheaper subsequent fetches, but drop it for
          // terminal PRs so the next cycle looks the branch up by name and can
          // discover a new PR opened on it.
          const info = result.value;
          if (info?.number != null) {
            if (info.state === "MERGED" || info.state === "CLOSED") {
              prNumberCache.delete(key);
            } else {
              prNumberCache.set(key, info.number);
            }
          }
        }
      } catch (err) {
        anyFailure = String(err);
        log("warn", "integration-cache", `Batch PR fetch failed for repo ${repoId}, keeping cached data: ${err}`);
      }
    })
  );

  prune("pr", allKeys, prCache);
  health.githubFailing = anyFailure !== null;
  health.lastGithubError = anyFailure;
}

/**
 * Refresh cached Linear info for every branch in `repoGroups`. Branches whose fetch
 * failed keep whatever they already had — this is what stops the list going
 * ungrouped when the network drops.
 */
export async function refreshLinearCache(
  repoGroups: RepoBranches[],
  apiKey: string
): Promise<void> {
  if (repoGroups.length === 0) return;
  hydrateIntegrationCache();

  const targets: Array<{ key: string; branch: string }> = [];
  for (const { repoId, branches } of repoGroups) {
    for (const branch of branches) targets.push({ key: cacheKey(repoId, branch), branch });
  }
  if (targets.length === 0) return;

  let anyFailure: string | null = null;

  // Linear's issueVcsBranchSearch is workspace-wide and takes no repo argument, so
  // the same branch in two repos resolves to the same ticket. Fetch once per
  // distinct branch and fan the answer out to each repo-scoped key.
  const distinctBranches = [...new Set(targets.map((t) => t.branch))];
  const byBranch = new Map<string, FetchResult<LinearInfo>>();
  await Promise.all(
    distinctBranches.map(async (branch) => {
      byBranch.set(branch, await fetchLinearResult(branch, apiKey));
    })
  );

  for (const { key, branch } of targets) {
    const result = byBranch.get(branch);
    if (!result) continue;
    if (!result.ok) {
      anyFailure = result.error;
      continue;
    }
    commit("linear", key, linearCache, result);
  }

  prune("linear", targets.map((t) => t.key), linearCache);
  health.linearFailing = anyFailure !== null;
  health.lastLinearError = anyFailure;
}

/**
 * Resolve the cached `pr_info` / `linear_info` fields for one worktree. `pr_info`
 * prefers the gh-sourced PR and falls back to the PR attached to the Linear ticket.
 */
export function integrationFieldsFor(
  repoId: string,
  branch: string
): { pr_info: PrInfo | null; linear_info: LinearInfo | null } {
  hydrateIntegrationCache();
  const key = cacheKey(repoId, branch);
  const linear_info = linearCache.get(key) ?? null;
  const ghPr = prCache.get(key);
  if (ghPr) return { pr_info: ghPr, linear_info };
  if (linear_info?.prAttachment && linearAttachmentMatchesBranch(linear_info.prAttachment, branch)) {
    return { pr_info: linearAttachmentToPrInfo(linear_info.prAttachment), linear_info };
  }
  return { pr_info: null, linear_info };
}

/** Cached Linear ticket for a branch, used for auto-nicknaming. */
export function cachedLinearInfo(repoId: string, branch: string): LinearInfo | null {
  hydrateIntegrationCache();
  return linearCache.get(cacheKey(repoId, branch)) ?? null;
}

/** Forget every cached Linear ticket (memory + disk) — used when Linear is disabled. */
export function clearCachedLinear(): void {
  linearCache.clear();
  try {
    pruneIntegrationCache("linear", []);
  } catch (err) {
    log("warn", "integration-cache", `Failed to clear Linear cache: ${err}`);
  }
  health.linearFailing = false;
  health.lastLinearError = null;
}

export function getIntegrationHealth(): IntegrationHealth {
  return { ...health };
}

/** Test seam — drops in-memory state so the next call re-hydrates from the DB. */
export function resetIntegrationCache(): void {
  prCache.clear();
  linearCache.clear();
  prNumberCache.clear();
  hydrated = false;
  health.githubFailing = false;
  health.linearFailing = false;
  health.lastGithubError = null;
  health.lastLinearError = null;
}
