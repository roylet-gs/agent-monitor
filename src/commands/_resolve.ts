import { resolve } from "path";
import {
  getWorktreeByPath,
  getWorktreeByBranch,
  getRepositories,
  getAllWorktrees,
} from "../lib/db.js";
import type { Worktree } from "../lib/types.js";

export interface ResolveOptions {
  worktree?: string;
  branch?: string;
  repo?: string;
}

/**
 * Resolve a worktree from CLI flags or cwd.
 * Priority: --worktree path > --branch name > auto-detect from cwd.
 */
export function resolveWorktree(opts: ResolveOptions): Worktree {
  // 1. Explicit path
  if (opts.worktree) {
    const absPath = resolve(opts.worktree);
    const wt = getWorktreeByPath(absPath);
    if (!wt) {
      console.error(`Error: no worktree found in DB for path: ${absPath}`);
      process.exit(1);
    }
    return wt;
  }

  // 2. Branch name (optionally scoped by --repo)
  if (opts.branch) {
    if (opts.repo) {
      const repos = getRepositories();
      const repoPath = resolve(opts.repo);
      const repo = repos.find((r) => r.path === repoPath);
      if (!repo) {
        console.error(`Error: repository not found: ${repoPath}`);
        process.exit(1);
      }
      const wt = getWorktreeByBranch(repo.id, opts.branch);
      if (!wt) {
        console.error(`Error: no worktree found for branch "${opts.branch}" in repo ${repo.name}`);
        process.exit(1);
      }
      return wt;
    }

    // Search across all repos
    const allWorktrees = getAllWorktrees();
    const matches = allWorktrees.filter((w) => w.branch === opts.branch);
    if (matches.length === 0) {
      console.error(`Error: no worktree found for branch "${opts.branch}"`);
      process.exit(1);
    }
    if (matches.length > 1) {
      console.error(`Error: branch "${opts.branch}" exists in multiple repos. Use --repo to disambiguate.`);
      process.exit(1);
    }
    return matches[0]!;
  }

  // 3. Auto-detect from cwd
  const cwd = process.cwd();
  const wt = getWorktreeByPath(cwd);
  if (!wt) {
    console.error("Error: not inside a known worktree. Use --worktree or --branch to specify one.");
    process.exit(1);
  }
  return wt;
}
