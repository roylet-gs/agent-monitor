import { getWorktrees, getAgentStatuses } from "./db.js";
import { getGitStatus, getLastCommit } from "./git.js";
import { fetchPrInfo } from "./github.js";
import { fetchLinearInfo } from "./linear.js";
import { log } from "./logger.js";
import type {
  Worktree,
  WorktreeWithStatus,
  WorktreeGroup,
  Repository,
  Settings,
  PrInfo,
  LinearInfo,
} from "./types.js";

export interface EnrichOptions {
  ghPrStatus?: boolean;
  linearEnabled?: boolean;
  linearApiKey?: string;
  prCache?: Map<string, PrInfo | null>;
  linearCache?: Map<string, LinearInfo | null>;
}

/**
 * Enrich a single worktree with git status, PR info, Linear info, and agent status.
 */
export async function enrichWorktree(
  wt: Worktree,
  agentStatusMap: Map<string, import("./types.js").AgentStatus>,
  repoPath: string,
  opts: EnrichOptions = {}
): Promise<WorktreeWithStatus> {
  let git_status = null;
  let last_commit = null;
  try {
    [git_status, last_commit] = await Promise.all([
      getGitStatus(wt.path),
      getLastCommit(wt.path),
    ]);
  } catch (err) {
    log("warn", "enrich", `Failed to get git info for ${wt.path}: ${err}`);
  }

  let pr_info: PrInfo | null = opts.prCache?.get(wt.branch) ?? null;
  if (!pr_info && opts.ghPrStatus) {
    try {
      pr_info = await fetchPrInfo(repoPath, wt.branch);
    } catch {
      // ignore
    }
  }

  let linear_info: LinearInfo | null = opts.linearCache?.get(wt.branch) ?? null;
  if (!linear_info && opts.linearEnabled && opts.linearApiKey) {
    try {
      linear_info = await fetchLinearInfo(wt.branch, opts.linearApiKey);
    } catch {
      // ignore
    }
  }

  return {
    ...wt,
    agent_status: agentStatusMap.get(wt.id) ?? null,
    git_status,
    last_commit,
    pr_info,
    linear_info,
  };
}

/**
 * Enrich all worktrees across multiple repositories.
 */
export async function enrichAllWorktrees(
  repos: Repository[],
  settings: Settings
): Promise<WorktreeGroup[]> {
  const groups: WorktreeGroup[] = [];
  const opts: EnrichOptions = {
    ghPrStatus: settings.ghPrStatus,
    linearEnabled: settings.linearEnabled,
    linearApiKey: settings.linearApiKey,
  };

  for (const repo of repos) {
    const dbWorktrees = getWorktrees(repo.id);
    const statuses = getAgentStatuses(repo.id);

    const enriched = await Promise.all(
      dbWorktrees.map((wt) => enrichWorktree(wt, statuses, repo.path, opts))
    );

    enriched.sort((a, b) => b.created_at.localeCompare(a.created_at));
    groups.push({ repo, worktrees: enriched });
  }

  return groups;
}
