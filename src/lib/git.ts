import { simpleGit, type SimpleGit } from "simple-git";
import { execSync } from "child_process";
import { existsSync, readFileSync, rmSync } from "fs";
import { basename, join, resolve } from "path";
import { log } from "./logger.js";
import type { GitStatus, CommitInfo } from "./types.js";

export function getGit(cwd: string): SimpleGit {
  return simpleGit(cwd);
}

export function isGitRepo(path: string): boolean {
  return existsSync(join(path, ".git"));
}

export function getWorktreeRoot(cwd: string): string | undefined {
  try {
    const result = execSync("git rev-parse --show-toplevel", {
      cwd,
      encoding: "utf-8",
      stdio: ["pipe", "pipe", "pipe"],
    });
    return result.trim();
  } catch {
    return undefined;
  }
}

export interface GitWorktreeInfo {
  path: string;
  branch: string;
  isMain: boolean;
}

export function recoverDetachedBranch(worktreePath: string): string | undefined {
  try {
    const gitDir = execSync("git rev-parse --git-dir", {
      cwd: worktreePath,
      encoding: "utf-8",
      stdio: ["pipe", "pipe", "pipe"],
    }).trim();

    const resolvedGitDir = resolve(worktreePath, gitDir);

    // Check rebase-merge (interactive rebase) then rebase-apply (git am / older rebase)
    for (const dir of ["rebase-merge", "rebase-apply"]) {
      const headNamePath = join(resolvedGitDir, dir, "head-name");
      if (existsSync(headNamePath)) {
        const ref = readFileSync(headNamePath, "utf-8").trim();
        const branch = ref.replace("refs/heads/", "");
        log("debug", "git", `Recovered branch "${branch}" from ${dir}/head-name for ${worktreePath}`);
        return branch;
      }
    }
  } catch (err) {
    log("debug", "git", `Failed to recover detached branch for ${worktreePath}: ${err}`);
  }
  return undefined;
}

export async function listWorktrees(repoPath: string): Promise<GitWorktreeInfo[]> {
  const git = getGit(repoPath);
  try {
    const result = await git.raw(["worktree", "list", "--porcelain"]);
    const worktrees: GitWorktreeInfo[] = [];
    let currentPath: string | undefined;
    let currentBranch: string | undefined;
    let isDetached = false;

    const finishEntry = () => {
      if (currentPath && currentBranch) {
        worktrees.push({
          path: currentPath,
          branch: currentBranch,
          isMain: currentPath === resolve(repoPath),
        });
      } else if (currentPath && !currentBranch && isDetached) {
        const recovered = recoverDetachedBranch(currentPath);
        if (recovered) {
          worktrees.push({
            path: currentPath,
            branch: recovered,
            isMain: currentPath === resolve(repoPath),
          });
        } else {
          log("warn", "git", `Skipping detached worktree at ${currentPath} (could not recover branch)`);
        }
      }
      currentPath = undefined;
      currentBranch = undefined;
      isDetached = false;
    };

    for (const line of result.split("\n")) {
      if (line.startsWith("worktree ")) {
        finishEntry(); // flush previous entry if blank line separator was missing
        currentPath = line.slice(9).trim();
      } else if (line.startsWith("branch ")) {
        const ref = line.slice(7).trim();
        currentBranch = ref.replace("refs/heads/", "");
      } else if (line === "detached") {
        isDetached = true;
      } else if (line === "") {
        finishEntry();
      }
    }
    // handle last entry without trailing newline
    finishEntry();

    return worktrees;
  } catch (err) {
    log("error", "git", `Failed to list worktrees: ${err}`);
    return [];
  }
}

export interface CreateWorktreeOptions {
  baseRef?: string;
  reuse?: boolean;
  // Pass --no-track so the new branch has no upstream even when a remote
  // branch with the same name exists (git's DWIM would otherwise auto-track).
  noTrack?: boolean;
  // Pass --track to set up upstream tracking from the commit-ish (baseRef).
  // Use this together with `baseRef: "origin/<branch>"` to create a local
  // branch tracking the remote.
  track?: boolean;
}

export async function createWorktree(
  repoPath: string,
  branch: string,
  opts: CreateWorktreeOptions = {}
): Promise<string> {
  const git = getGit(repoPath);
  const { baseRef, reuse = false, noTrack = false, track = false } = opts;
  // worktrees go into .claude/worktrees/ directory next to .git
  const worktreePath = join(repoPath, ".claude", "worktrees", branch.replace(/\//g, "-"));

  const args = ["worktree", "add", worktreePath];
  if (reuse) {
    args.push(branch);
  } else {
    args.push("-b", branch);
    if (track) {
      args.push("--track");
    } else if (noTrack) {
      args.push("--no-track");
    }
    if (baseRef) {
      args.push(baseRef);
    }
  }

  await git.raw(args);
  log("info", "git", `Created worktree at ${worktreePath} for branch ${branch}`);
  return worktreePath;
}

export async function deleteWorktree(repoPath: string, worktreePath: string, force = false): Promise<void> {
  const git = getGit(repoPath);
  const args = ["worktree", "remove", worktreePath];
  if (force) args.push("--force");
  try {
    await git.raw(args);
  } catch (err) {
    // git worktree remove --force can still fail with "Directory not empty"
    // when untracked files exist (e.g. .claude/ dirs). Fall back to manual cleanup.
    if (force && existsSync(worktreePath)) {
      log("warn", "git", `git worktree remove failed, removing directory manually: ${err}`);
      rmSync(worktreePath, { recursive: true, force: true });
      await git.raw(["worktree", "prune"]);
    } else {
      throw err;
    }
  }
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

export async function checkoutBranch(repoPath: string, branch: string): Promise<void> {
  const git = getGit(repoPath);
  await git.raw(["checkout", branch]);
  log("info", "git", `Checked out branch ${branch} in ${repoPath}`);
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
      return getCurrentBranch(repoPath);
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

export async function localBranchExists(repoPath: string, branch: string): Promise<boolean> {
  const git = getGit(repoPath);
  try {
    await git.raw(["rev-parse", "--verify", `refs/heads/${branch}`]);
    return true;
  } catch {
    return false;
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

// Authoritative remote check via `git ls-remote`. Detects branches the user
// has not yet fetched. Falls back to the cached refs/remotes/origin check on
// network failure (no origin configured, offline, timeout).
export async function lsRemoteBranch(
  repoPath: string,
  branch: string,
  timeoutMs = 3000
): Promise<boolean> {
  const args = ["ls-remote", "--exit-code", "--heads", "origin", `refs/heads/${branch}`];
  try {
    const output = execSync(`git ${args.map((a) => JSON.stringify(a)).join(" ")}`, {
      cwd: repoPath,
      encoding: "utf-8",
      stdio: ["pipe", "pipe", "pipe"],
      timeout: timeoutMs,
    });
    return output.trim().length > 0;
  } catch (err: unknown) {
    const e = err as { status?: number; signal?: string; code?: string };
    // `ls-remote --exit-code` returns 2 when no matching ref found — that's
    // an authoritative "no", not a failure.
    if (e?.status === 2) return false;
    log(
      "warn",
      "git",
      `ls-remote failed for ${branch} (falling back to cached ref): ${err}`
    );
    return remoteBranchExists(repoPath, branch);
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

export async function fetchAndResetBranch(repoPath: string, branch: string): Promise<boolean> {
  const git = getGit(repoPath);
  try {
    await git.raw(["fetch", "origin", branch]);
    log("info", "git", `Fetched origin/${branch} for reuse`);
  } catch (err) {
    log("warn", "git", `Failed to fetch origin/${branch} (using local branch as-is): ${err}`);
    return false;
  }

  try {
    // Check if the remote tracking ref actually exists after fetch
    await git.raw(["rev-parse", "--verify", `refs/remotes/origin/${branch}`]);
    // Reset local branch to match remote (creates the local ref if missing)
    await git.raw(["branch", "-f", branch, `origin/${branch}`]);
    // `branch -f` does not configure upstream tracking — set it explicitly
    // so push/pull just work in the worktree.
    try {
      await git.raw(["branch", "--set-upstream-to", `origin/${branch}`, branch]);
    } catch (err) {
      log("debug", "git", `Failed to set upstream for ${branch}: ${err}`);
    }
    log("info", "git", `Reset local branch ${branch} to origin/${branch}`);
    return true;
  } catch (err) {
    log("debug", "git", `No remote branch origin/${branch}, using local as-is: ${err}`);
    return false;
  }
}

export async function getCurrentBranch(repoPath: string): Promise<string> {
  const git = getGit(repoPath);
  // symbolic-ref works on empty repos (no commits yet); rev-parse --abbrev-ref HEAD does not.
  try {
    const ref = await git.raw(["symbolic-ref", "--short", "HEAD"]);
    return ref.trim();
  } catch {
    // Detached HEAD — fall back to rev-parse, which returns "HEAD" in that case.
    const branch = await git.raw(["rev-parse", "--abbrev-ref", "HEAD"]);
    return branch.trim();
  }
}

export interface EnsureBranchResult {
  ready: boolean;
  switched?: boolean;
  error?: string;
}

export async function ensureBranchForOpen(
  worktreePath: string,
  expectedBranch: string,
  isMain: boolean
): Promise<EnsureBranchResult> {
  if (!isMain) return { ready: true };

  const defaultBranch = await getMainBranch(worktreePath);
  if (expectedBranch === defaultBranch) return { ready: true };

  const current = await getCurrentBranch(worktreePath);
  if (current === expectedBranch) return { ready: true };

  const exists = await branchExists(worktreePath, expectedBranch);
  if (!exists) {
    return { ready: false, error: `Branch "${expectedBranch}" does not exist.` };
  }

  const status = await getGitStatus(worktreePath);
  if (status.dirty > 0) {
    return {
      ready: false,
      error: `Cannot switch to branch ${expectedBranch}: ${status.dirty} uncommitted change${status.dirty === 1 ? "" : "s"}. Stash or commit first.`,
    };
  }

  await checkoutBranch(worktreePath, expectedBranch);
  return { ready: true, switched: true };
}

export function getRepoName(repoPath: string): string {
  return basename(resolve(repoPath));
}
