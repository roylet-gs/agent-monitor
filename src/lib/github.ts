import { execFile, execFileSync } from "child_process";
import { log } from "./logger.js";
import type { PrInfo } from "./types.js";

interface GhPrResult {
  number: number;
  title: string;
  url: string;
  state: string;
  isDraft: boolean;
  reviewDecision: string;
  statusCheckRollup: Array<{
    status: string;
    conclusion: string;
    detailsUrl?: string;
    name?: string;
  }>;
}

const GH_PR_FIELDS = "number,title,url,state,isDraft,reviewDecision,statusCheckRollup";

function execGh(args: string[], cwd: string, timeout = 5000): Promise<string> {
  return new Promise((resolve, reject) => {
    execFile("gh", args, { cwd, timeout }, (err, stdout) => {
      if (err) {
        reject(err);
      } else {
        resolve(stdout);
      }
    });
  });
}

// --- Per-repo exponential backoff state ---
interface BackoffState {
  backoffUntil: number;
  backoffMs: number;
}
const repoBackoff = new Map<string, BackoffState>();
const skipLoggedForRepo = new Set<string>();
const BACKOFF_INITIAL_MS = 5_000;
const BACKOFF_MAX_MS = 300_000; // 5 minutes

function isInBackoff(repoPath: string): boolean {
  const state = repoBackoff.get(repoPath);
  if (!state || state.backoffUntil === 0) return false;
  if (Date.now() >= state.backoffUntil) {
    // Backoff period expired, allow retry
    skipLoggedForRepo.delete(repoPath);
    return false;
  }
  return true;
}

function onGhSuccess(repoPath: string): void {
  const state = repoBackoff.get(repoPath);
  if (state && state.backoffMs > 0) {
    log("info", "github", `GitHub API recovered for ${repoPath}, resetting backoff`);
  }
  repoBackoff.delete(repoPath);
  skipLoggedForRepo.delete(repoPath);
}

function onGhFailure(repoPath: string, err: unknown): void {
  const state = repoBackoff.get(repoPath);
  const prevMs = state?.backoffMs ?? 0;
  const newMs = prevMs === 0 ? BACKOFF_INITIAL_MS : Math.min(prevMs * 2, BACKOFF_MAX_MS);
  repoBackoff.set(repoPath, { backoffUntil: Date.now() + newMs, backoffMs: newMs });
  log("warn", "github", `GitHub API error for ${repoPath}, backing off for ${newMs / 1000}s: ${err}`);
}

export function deriveChecksStatus(
  checks: Array<{ status: string; conclusion: string }>
): PrInfo["checksStatus"] {
  if (!checks || checks.length === 0) return "none";
  const hasFailure = checks.some(
    (c) => c.conclusion === "FAILURE" || c.conclusion === "ERROR" || c.conclusion === "CANCELLED"
  );
  if (hasFailure) return "failing";
  const hasPending = checks.some(
    (c) => c.status !== "COMPLETED"
  );
  if (hasPending) return "pending";
  return "passing";
}

function ghResultToPrInfo(pr: GhPrResult): PrInfo {
  const checks = pr.statusCheckRollup ?? [];
  const checksStatus = deriveChecksStatus(checks);

  // Find the most relevant active check (failing first, then pending/waiting)
  const failingCheck = checks.find(
    (c) => c.conclusion === "FAILURE" || c.conclusion === "ERROR" || c.conclusion === "CANCELLED"
  );
  const pendingCheck = checks.find(
    (c) => c.status !== "COMPLETED"
  );
  const activeCheck = failingCheck ?? pendingCheck;

  const checksWaiting = checks.some(
    (c) => c.status === "WAITING" || c.conclusion === "ACTION_REQUIRED"
  );

  return {
    number: pr.number,
    title: pr.title,
    url: pr.url,
    state: pr.state,
    isDraft: pr.isDraft,
    reviewDecision: pr.reviewDecision ?? "",
    hasFeedback: false,
    checksStatus,
    activeCheckUrl: activeCheck?.detailsUrl ?? null,
    activeCheckName: activeCheck?.name ?? null,
    checksWaiting,
  };
}

/**
 * Returns true if a cached PR is in a terminal state that doesn't need re-fetching.
 */
export function shouldSkipPrFetch(cached: PrInfo | null): boolean {
  if (!cached) return false;
  if (cached.state === "CLOSED") return true;
  if (cached.state === "MERGED") {
    return cached.checksStatus === "passing" || cached.checksStatus === "none";
  }
  return false;
}

/**
 * Fetch PR info for a single branch using `gh pr view <branch>`.
 * If prNumber is provided, uses `gh pr view <number>` for a cheaper lookup.
 */
export async function fetchPrInfo(
  repoPath: string,
  branch: string,
  prNumber?: number
): Promise<PrInfo | null> {
  if (isInBackoff(repoPath)) {
    if (!skipLoggedForRepo.has(repoPath + ":" + branch)) {
      log("debug", "github", `Skipping PR fetch for ${branch} (in backoff)`);
      skipLoggedForRepo.add(repoPath + ":" + branch);
    }
    return null;
  }

  try {
    const target = prNumber != null ? String(prNumber) : branch;
    const stdout = await execGh(
      ["pr", "view", target, "--json", GH_PR_FIELDS],
      repoPath
    );

    const pr: GhPrResult = JSON.parse(stdout);
    onGhSuccess(repoPath);
    return ghResultToPrInfo(pr);
  } catch (err) {
    const msg = String(err);
    // "no pull requests found" is not an API error, just means no PR exists
    if (msg.includes("no pull requests found") || msg.includes("Could not resolve")) {
      onGhSuccess(repoPath);
      return null;
    }
    onGhFailure(repoPath, err);
    return null;
  }
}

/**
 * Fetch PR info for multiple branches with per-branch `gh pr view` calls.
 * Concurrency-limited to avoid overwhelming the API.
 * prNumberCache maps branch -> known PR number for cheaper lookups.
 */
export async function fetchAllPrInfo(
  repoPath: string,
  branches: string[],
  prNumberCache?: Map<string, number>,
  prCache?: Map<string, PrInfo | null>
): Promise<Map<string, PrInfo | null>> {
  const result = new Map<string, PrInfo | null>();
  if (branches.length === 0) return result;

  if (isInBackoff(repoPath)) {
    if (!skipLoggedForRepo.has(repoPath)) {
      log("debug", "github", `Skipping all PR fetches for ${repoPath} (in backoff)`);
      skipLoggedForRepo.add(repoPath);
    }
    for (const b of branches) result.set(b, null);
    return result;
  }

  // Determine which branches need fetching and how
  const branchesToFetch: string[] = [];
  for (const branch of branches) {
    const cached = prCache?.get(branch) ?? null;
    if (shouldSkipPrFetch(cached)) {
      // Terminal PRs still need a branch-name fetch to discover new PRs,
      // but we clear the cached PR number so we don't just re-fetch the old one.
      log("debug", "github", `Re-fetching ${branch} by name (cached PR #${cached!.number} is ${cached!.state})`);
      prNumberCache?.delete(branch);
    }
    branchesToFetch.push(branch);
  }

  const CONCURRENCY = 3;
  let i = 0;

  async function next(): Promise<void> {
    while (i < branchesToFetch.length) {
      const branch = branchesToFetch[i++]!;
      const knownNumber = prNumberCache?.get(branch);
      const info = await fetchPrInfo(repoPath, branch, knownNumber);
      result.set(branch, info);
      // If we entered backoff during this batch, fill remaining with null
      if (isInBackoff(repoPath)) {
        while (i < branchesToFetch.length) {
          result.set(branchesToFetch[i++]!, null);
        }
        return;
      }
    }
  }

  // Launch up to CONCURRENCY workers
  const workers: Promise<void>[] = [];
  for (let w = 0; w < Math.min(CONCURRENCY, branchesToFetch.length); w++) {
    workers.push(next());
  }
  await Promise.all(workers);

  return result;
}

export function getPrStatusLabel(pr: PrInfo): { label: string; color: string } {
  const { state, reviewDecision, hasFeedback, isDraft, checksStatus, checksWaiting } = pr;

  if (state === "MERGED") {
    if (checksStatus === "failing") {
      return { label: "Merged - Actions Failing", color: "red" };
    }
    if (checksWaiting) {
      return { label: "Merged - Awaiting Approval", color: "yellow" };
    }
    if (checksStatus === "pending") {
      return { label: "Merged - Actions Running", color: "magenta" };
    }
    return { label: "Merged", color: "magenta" };
  }
  if (state === "CLOSED") {
    return { label: "Closed", color: "red" };
  }

  if (reviewDecision === "CHANGES_REQUESTED") {
    return { label: "Changes Requested", color: "red" };
  }

  if (reviewDecision === "APPROVED") {
    if (checksStatus === "failing") {
      return { label: "Approved - Checks Failing", color: "yellow" };
    }
    if (checksStatus === "pending") {
      return { label: "Approved - Checks Running", color: "cyan" };
    }
    return { label: "Approved", color: "green" };
  }

  // REVIEW_REQUIRED or empty
  if (isDraft) {
    if (hasFeedback) {
      return { label: "Draft - Feedback", color: "yellow" };
    }
    return { label: "Draft", color: "gray" };
  }

  if (hasFeedback) {
    return { label: "In Review - Feedback", color: "yellow" };
  }
  if (checksStatus === "failing") {
    return { label: "In Review - Checks Failing", color: "yellow" };
  }
  if (checksStatus === "pending") {
    return { label: "In Review - Checks Running", color: "cyan" };
  }
  return { label: "In Review", color: "cyan" };
}

export function isGhAvailable(): boolean {
  try {
    execFileSync("gh", ["--version"], { timeout: 3000, stdio: "ignore" });
    return true;
  } catch {
    return false;
  }
}
