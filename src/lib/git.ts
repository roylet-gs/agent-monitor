import { simpleGit, type SimpleGit } from "simple-git";
import { existsSync } from "fs";
import { basename, join, resolve } from "path";
import { log } from "./logger.js";
import type { GitStatus, CommitInfo } from "./types.js";

export function getGit(cwd: string): SimpleGit {
  return simpleGit(cwd);
}

export function isGitRepo(path: string): boolean {
  return existsSync(join(path, ".git"));
}

export interface GitWorktreeInfo {
  path: string;
  branch: string;
  isMain: boolean;
}

export async function listWorktrees(repoPath: string): Promise<GitWorktreeInfo[]> {
  const git = getGit(repoPath);
  try {
    const result = await git.raw(["worktree", "list", "--porcelain"]);
    const worktrees: GitWorktreeInfo[] = [];
    let current: Partial<GitWorktreeInfo> = {};

    for (const line of result.split("\n")) {
      if (line.startsWith("worktree ")) {
        current.path = line.slice(9).trim();
      } else if (line.startsWith("branch ")) {
        // branch refs/heads/feature/foo → feature/foo
        const ref = line.slice(7).trim();
        current.branch = ref.replace("refs/heads/", "");
      } else if (line === "") {
        if (current.path && current.branch) {
          worktrees.push({
            path: current.path,
            branch: current.branch,
            isMain: current.path === resolve(repoPath),
          });
        }
        current = {};
      }
    }
    // handle last entry without trailing newline
    if (current.path && current.branch) {
      worktrees.push({
        path: current.path,
        branch: current.branch,
        isMain: current.path === resolve(repoPath),
      });
    }

    return worktrees;
  } catch (err) {
    log("error", "git", `Failed to list worktrees: ${err}`);
    return [];
  }
}

export async function createWorktree(
  repoPath: string,
  branch: string,
  baseBranch?: string,
  reuseExisting = false
): Promise<string> {
  const git = getGit(repoPath);
  // worktrees go into .worktrees/ directory next to .git
  const worktreePath = join(repoPath, ".worktrees", branch.replace(/\//g, "-"));

  const args = ["worktree", "add", worktreePath];
  if (reuseExisting) {
    args.push(branch);
  } else if (baseBranch) {
    args.push("-b", branch, baseBranch);
  } else {
    args.push("-b", branch);
  }

  await git.raw(args);
  log("info", "git", `Created worktree at ${worktreePath} for branch ${branch}`);
  return worktreePath;
}

export async function deleteWorktree(repoPath: string, worktreePath: string, force = false): Promise<void> {
  const git = getGit(repoPath);
  const args = ["worktree", "remove", worktreePath];
  if (force) args.push("--force");
  await git.raw(args);
  log("info", "git", `Deleted worktree at ${worktreePath}`);
}

export async function getGitStatus(worktreePath: string): Promise<GitStatus> {
  const git = getGit(worktreePath);
  try {
    const status = await git.status();
    // Get ahead/behind
    let ahead = 0;
    let behind = 0;
    try {
      const result = await git.raw(["rev-list", "--left-right", "--count", "HEAD...@{upstream}"]);
      const parts = result.trim().split(/\s+/);
      ahead = parseInt(parts[0] ?? "0", 10);
      behind = parseInt(parts[1] ?? "0", 10);
    } catch {
      // no upstream tracking — that's fine
    }

    return {
      ahead,
      behind,
      dirty: status.files.length,
    };
  } catch (err) {
    log("warn", "git", `Failed to get status for ${worktreePath}: ${err}`);
    return { ahead: 0, behind: 0, dirty: 0 };
  }
}

export async function getLastCommit(worktreePath: string): Promise<CommitInfo | null> {
  const git = getGit(worktreePath);
  try {
    const logResult = await git.log({ maxCount: 1 });
    const latest = logResult.latest;
    if (!latest) return null;

    // Get relative time
    const relTime = await git.raw(["log", "-1", "--format=%cr"]);

    return {
      hash: latest.hash.slice(0, 7),
      message: latest.message,
      relative_time: relTime.trim(),
    };
  } catch {
    return null;
  }
}

export async function getMainBranch(repoPath: string): Promise<string> {
  const git = getGit(repoPath);
  try {
    // Check if main exists
    await git.raw(["rev-parse", "--verify", "main"]);
    return "main";
  } catch {
    try {
      await git.raw(["rev-parse", "--verify", "master"]);
      return "master";
    } catch {
      // Fall back to current branch
      const branch = await git.raw(["rev-parse", "--abbrev-ref", "HEAD"]);
      return branch.trim();
    }
  }
}

export async function branchExists(repoPath: string, branch: string): Promise<boolean> {
  const git = getGit(repoPath);
  try {
    await git.raw(["rev-parse", "--verify", `refs/heads/${branch}`]);
    return true;
  } catch {
    // Also check if a remote tracking branch exists, since
    // `git worktree add -b <branch>` will fail if origin/<branch> exists
    try {
      await git.raw(["rev-parse", "--verify", `refs/remotes/origin/${branch}`]);
      return true;
    } catch {
      return false;
    }
  }
}

export async function remoteBranchExists(repoPath: string, branch: string): Promise<boolean> {
  const git = getGit(repoPath);
  try {
    await git.raw(["rev-parse", "--verify", `refs/remotes/origin/${branch}`]);
    return true;
  } catch {
    return false;
  }
}

export async function deleteBranch(repoPath: string, branch: string, force = false): Promise<void> {
  const git = getGit(repoPath);
  const flag = force ? "-D" : "-d";
  await git.raw(["branch", flag, branch]);
  log("info", "git", `Deleted local branch ${branch}`);
}

export async function deleteRemoteBranch(repoPath: string, branch: string): Promise<void> {
  const git = getGit(repoPath);
  await git.raw(["push", "origin", "--delete", branch]);
  log("info", "git", `Deleted remote branch origin/${branch}`);
}

export async function fetchBranch(repoPath: string, branch: string): Promise<void> {
  const git = getGit(repoPath);
  try {
    await git.raw(["fetch", "origin", branch]);
    log("info", "git", `Fetched origin/${branch}`);
  } catch (err) {
    log("warn", "git", `Failed to fetch origin/${branch}: ${err}`);
  }
}

export function getRepoName(repoPath: string): string {
  return basename(resolve(repoPath));
}
