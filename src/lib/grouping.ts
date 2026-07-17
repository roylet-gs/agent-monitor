import { isEffectivelyOpen } from "./agent-utils.js";
import { DEFAULT_SETTINGS } from "./settings.js";
import type {
  Repository,
  WorktreeGroup,
  WorktreeSortCriterion,
  WorktreeSortKey,
  WorktreeWithStatus,
} from "./types.js";

export interface RepoWorktrees {
  repo: Repository;
  worktrees: WorktreeWithStatus[];
}

type Cmp = (a: WorktreeWithStatus, b: WorktreeWithStatus) => number;

// Agent status ordering, active-first (lower rank = higher priority).
const AGENT_RANK: Record<string, number> = {
  executing: 0,
  planning: 1,
  waiting: 2,
  idle: 3,
  done: 4,
  none: 5,
};
const agentRank = (w: WorktreeWithStatus): number =>
  AGENT_RANK[w.agent_status?.status ?? "none"] ?? 5;

// PR ordering, attention-first: failing checks and requested changes float up,
// merged/closed sink, worktrees without a PR go last.
function prRank(w: WorktreeWithStatus): number {
  const pr = w.pr_info;
  if (!pr) return 7;
  if (pr.checksStatus === "failing") return 0;
  if (pr.hasFeedback || pr.reviewDecision === "CHANGES_REQUESTED") return 1;
  if (pr.checksStatus === "pending" || pr.checksWaiting) return 2;
  if (pr.state === "OPEN") return pr.isDraft ? 4 : 3;
  if (pr.state === "MERGED") return 5;
  return 6; // CLOSED (or any other terminal state)
}

// A present-before-absent string comparator (used for Linear ticket/project):
// worktrees carrying the value sort before those without it, then localeCompare.
function presentFirst(av: string, bv: string): number {
  if (av === bv) return 0;
  if (av && !bv) return -1;
  if (!av && bv) return 1;
  return av.localeCompare(bv);
}

// Each entry is an *ascending* comparator; makeComparator applies direction by
// negating the result for "desc" criteria.
const SORT_REGISTRY: Record<WorktreeSortKey, Cmp> = {
  isMain: (a, b) => a.is_main - b.is_main,
  // Repo is a structural (outer) dimension handled by buildGroups ordering the
  // repo sections — within a single repo group every worktree shares a repo, so
  // this is a no-op at the worktree-comparator level.
  repo: () => 0,
  linearTicket: (a, b) =>
    presentFirst(a.linear_info?.identifier ?? "", b.linear_info?.identifier ?? ""),
  linearProject: (a, b) =>
    presentFirst(a.linear_info?.project?.name ?? "", b.linear_info?.project?.name ?? ""),
  agentStatus: (a, b) => agentRank(a) - agentRank(b),
  lastActivity: (a, b) =>
    (a.agent_status?.updated_at ?? "").localeCompare(b.agent_status?.updated_at ?? ""),
  createdAt: (a, b) => a.created_at.localeCompare(b.created_at),
  branchName: (a, b) => a.branch.localeCompare(b.branch),
  prStatus: (a, b) => prRank(a) - prRank(b),
  gitDirty: (a, b) =>
    ((a.git_status?.dirty ?? 0) > 0 ? 0 : 1) - ((b.git_status?.dirty ?? 0) > 0 ? 0 : 1),
};

/**
 * Build a worktree comparator from an ordered list of sort criteria. Only
 * enabled criteria participate; they are applied in order and the first
 * non-zero comparison decides. Direction "desc" negates the ascending result.
 */
export function makeComparator(criteria: WorktreeSortCriterion[]): Cmp {
  const active = criteria.filter((c) => c.enabled);
  return (a, b) => {
    for (const c of active) {
      const r = SORT_REGISTRY[c.key]?.(a, b) ?? 0;
      if (r !== 0) return c.direction === "desc" ? -r : r;
    }
    return 0;
  };
}

// Legacy comparator, preserved for callers/tests that want the default order.
export const compareWorktrees: Cmp = makeComparator(DEFAULT_SETTINGS.worktreeSort);

export interface WorktreeFilterOpts {
  hideMainBranch: boolean;
  hideMergedClosedPrs: boolean;
  hideIdleDoneAgents: boolean;
  hideWithoutLinearTicket: boolean;
}

/**
 * Apply the dashboard's worktree visibility filters. Centralized here so the
 * TUI hook and the daemon can never diverge. A worktree with an effectively-open
 * session is never hidden (you shouldn't lose the row you're working in).
 */
export function applyWorktreeFilters(
  worktrees: WorktreeWithStatus[],
  opts: WorktreeFilterOpts
): WorktreeWithStatus[] {
  return worktrees.filter((wt) => {
    const open = isEffectivelyOpen(wt.agent_status);
    if (
      opts.hideMainBranch &&
      wt.is_main === 1 &&
      (wt.branch === "main" || wt.branch === "master") &&
      !open
    ) {
      return false;
    }
    if (
      opts.hideMergedClosedPrs &&
      !open &&
      (wt.pr_info?.state === "MERGED" || wt.pr_info?.state === "CLOSED")
    ) {
      return false;
    }
    if (opts.hideIdleDoneAgents && !open) {
      const s = wt.agent_status?.status;
      if (s === "idle" || s === "done" || s === "none" || !s) return false;
    }
    if (opts.hideWithoutLinearTicket && !open && !wt.linear_info?.identifier) {
      return false;
    }
    return true;
  });
}

/**
 * Build the dashboard's group list (one per repo) and its flattened
 * counterpart. Worktrees within each repo are sorted by the user's
 * `sortCriteria` — clustering by Linear ticket, project, etc. is achieved
 * purely through the sort order rather than a separate grouping mechanism.
 *
 * Invariant: `flatWorktrees` is the concatenation of `groups[].worktrees`,
 * so flat order always matches the visual top-to-bottom row order.
 */
export function buildGroups(
  perRepo: RepoWorktrees[],
  sortCriteria: WorktreeSortCriterion[]
): { groups: WorktreeGroup[]; flatWorktrees: WorktreeWithStatus[] } {
  const groups: WorktreeGroup[] = [];
  const cmp = makeComparator(sortCriteria);

  // Repo sections keep config order unless a "repo" sort criterion is enabled,
  // in which case they are ordered by repo name (honoring its direction).
  const repoCrit = sortCriteria.find((c) => c.enabled && c.key === "repo");
  const orderedRepos = repoCrit
    ? [...perRepo].sort((a, b) => {
        const r = a.repo.name.localeCompare(b.repo.name);
        return repoCrit.direction === "desc" ? -r : r;
      })
    : perRepo;

  for (const { repo, worktrees } of orderedRepos) {
    const sorted = [...worktrees].sort(cmp);
    // Keep an empty group only in the single-repo case so a lone repo always
    // renders; with multiple repos, empty repos are hidden.
    if (sorted.length > 0 || perRepo.length === 1) {
      groups.push({ repo, worktrees: sorted });
    }
  }

  const flatWorktrees = groups.flatMap((g) => g.worktrees);
  return { groups, flatWorktrees };
}
