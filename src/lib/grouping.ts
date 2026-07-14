import { log } from "./logger.js";
import type { LinearProject, Repository, WorktreeGroup, WorktreeWithStatus } from "./types.js";

export interface RepoWorktrees {
  repo: Repository;
  worktrees: WorktreeWithStatus[];
}

// Sort order within a repo bucket: dedicated worktrees first, main worktree
// branches last; worktrees sharing a Linear ticket cluster together; newest first.
export function compareWorktrees(a: WorktreeWithStatus, b: WorktreeWithStatus): number {
  if (a.is_main !== b.is_main) return a.is_main - b.is_main;
  const aLinear = a.linear_info?.identifier ?? "";
  const bLinear = b.linear_info?.identifier ?? "";
  if (aLinear !== bLinear) {
    // Worktrees with Linear tickets sort before those without
    if (aLinear && !bLinear) return -1;
    if (!aLinear && bLinear) return 1;
    return aLinear.localeCompare(bLinear);
  }
  return b.created_at.localeCompare(a.created_at);
}

/**
 * Build the dashboard's group list and its flattened counterpart.
 *
 * When `groupByProject` is set, worktrees whose Linear ticket belongs to a
 * project are bucketed project-major: one section per project (sorted by
 * name), each containing one group per repo (in input repo order). Everything
 * else (no ticket, ticket without a project) falls through to a trailing
 * no-project section that preserves the legacy per-repo layout.
 *
 * Invariant: `flatWorktrees` is the concatenation of `groups[].worktrees`,
 * so flat order always matches the visual top-to-bottom row order.
 */
export function buildGroups(
  perRepo: RepoWorktrees[],
  opts: { groupByProject: boolean }
): { groups: WorktreeGroup[]; flatWorktrees: WorktreeWithStatus[] } {
  const groups: WorktreeGroup[] = [];

  // Sort within each repo once; partitioning below preserves this order.
  const sorted = perRepo.map(({ repo, worktrees }) => ({
    repo,
    worktrees: [...worktrees].sort(compareWorktrees),
  }));

  if (opts.groupByProject) {
    // projectId -> { project, buckets: repo index -> worktrees }
    const projectSections = new Map<
      string,
      { project: LinearProject; buckets: Map<number, WorktreeWithStatus[]> }
    >();
    const remainders: WorktreeWithStatus[][] = sorted.map(() => []);

    sorted.forEach(({ worktrees }, repoIdx) => {
      for (const wt of worktrees) {
        const project = wt.linear_info?.project;
        if (project?.id) {
          let section = projectSections.get(project.id);
          if (!section) {
            section = { project, buckets: new Map() };
            projectSections.set(project.id, section);
          }
          const bucket = section.buckets.get(repoIdx);
          if (bucket) bucket.push(wt);
          else section.buckets.set(repoIdx, [wt]);
        } else {
          remainders[repoIdx].push(wt);
        }
      }
    });

    const orderedProjects = [...projectSections.values()].sort((a, b) =>
      a.project.name.localeCompare(b.project.name)
    );

    for (const { project, buckets } of orderedProjects) {
      for (let repoIdx = 0; repoIdx < sorted.length; repoIdx++) {
        const worktrees = buckets.get(repoIdx);
        if (worktrees) {
          groups.push({ repo: sorted[repoIdx].repo, worktrees, project });
        }
      }
      log(
        "debug",
        "grouping",
        `Project section "${project.name}" (${project.id}): ${buckets.size} repo bucket(s)`
      );
    }

    // Trailing no-project section keeps the legacy per-repo layout. The
    // single-repo empty-group rule only applies when nothing rendered above.
    sorted.forEach(({ repo }, repoIdx) => {
      const worktrees = remainders[repoIdx];
      if (worktrees.length > 0 || (sorted.length === 1 && groups.length === 0)) {
        groups.push({ repo, worktrees });
      }
    });
  } else {
    for (const { repo, worktrees } of sorted) {
      if (worktrees.length > 0 || sorted.length === 1) {
        groups.push({ repo, worktrees });
      }
    }
  }

  const flatWorktrees = groups.flatMap((g) => g.worktrees);
  return { groups, flatWorktrees };
}
