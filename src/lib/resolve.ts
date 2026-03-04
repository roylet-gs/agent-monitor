/**
 * Resolve CLI targets (branch names, paths, repos) to DB entities.
 */

import { resolve, basename } from "path";
import { existsSync } from "fs";
import {
  getRepositories,
  getRepositoryByPath,
  getWorktrees,
  getWorktreeByPath,
  getAllWorktrees,
} from "./db.js";
import { isGitRepo } from "./git.js";
import type { Repository, Worktree } from "./types.js";

/**
 * Detect the repository from CWD by walking up to find .git,
 * then matching against the DB.
 */
export function detectRepo(cwd?: string): Repository | undefined {
  const startDir = resolve(cwd ?? process.cwd());
  let dir = startDir;

  while (true) {
    // Check if this directory is a git repo root
    if (isGitRepo(dir)) {
      const repo = getRepositoryByPath(dir);
      if (repo) return repo;
    }

    // Check if this is a worktree (has .git as a file, not a directory)
    const gitPath = resolve(dir, ".git");
    if (existsSync(gitPath)) {
      // Could be a worktree — try to find the main repo
      // Walk further up to find the .worktrees parent
      const parent = resolve(dir, "..");
      if (parent === dir) break;
      dir = parent;
      continue;
    }

    const parent = resolve(dir, "..");
    if (parent === dir) break;
    dir = parent;
  }

  return undefined;
}

/**
 * Resolve --repo flag: explicit path, or detect from CWD.
 */
export function resolveRepo(repoPath?: string): Repository {
  if (repoPath) {
    const absPath = resolve(repoPath);
    const repo = getRepositoryByPath(absPath);
    if (!repo) {
      console.error(`Repository not tracked: ${absPath}\nRun: am repo add ${absPath}`);
      process.exit(1);
    }
    return repo;
  }

  const repo = detectRepo();
  if (!repo) {
    console.error("Could not detect repository from CWD.\nSpecify --repo <path> or run from inside a tracked repo.");
    process.exit(1);
  }
  return repo;
}

/**
 * Resolve a worktree target: by path or branch name.
 * Searches within a specific repo, or across all repos.
 */
export function resolveWorktree(target: string, repoId?: string): Worktree {
  // Try as absolute path first
  const absTarget = resolve(target);
  const byPath = getWorktreeByPath(absTarget);
  if (byPath) {
    if (repoId && byPath.repo_id !== repoId) {
      console.error(`Worktree ${target} belongs to a different repository.`);
      process.exit(1);
    }
    return byPath;
  }

  // Try as branch name
  const worktrees = repoId ? getWorktrees(repoId) : getAllWorktrees();
  const byBranch = worktrees.filter((w) => w.branch === target);
  if (byBranch.length === 1) return byBranch[0]!;
  if (byBranch.length > 1) {
    console.error(`Ambiguous target "${target}" — matches ${byBranch.length} worktrees. Specify --repo or use a path.`);
    process.exit(1);
  }

  // Try partial branch match (e.g. "my-feature" matches "feature/my-feature")
  const partial = worktrees.filter((w) => w.branch.endsWith(`/${target}`) || w.name === target);
  if (partial.length === 1) return partial[0]!;
  if (partial.length > 1) {
    console.error(`Ambiguous target "${target}" — matches ${partial.length} worktrees. Be more specific.`);
    process.exit(1);
  }

  console.error(`Worktree not found: ${target}`);
  process.exit(1);
}
