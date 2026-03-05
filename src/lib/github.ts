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
  statusCheckRollup: Array<{ status: string; conclusion: string }>;
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

function deriveChecksStatus(
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
  return {
    number: pr.number,
    title: pr.title,
    url: pr.url,
    state: pr.state,
    isDraft: pr.isDraft,
    reviewDecision: pr.reviewDecision ?? "",
    hasFeedback: false,
    checksStatus: deriveChecksStatus(pr.statusCheckRollup ?? []),
  };
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
  prNumberCache?: Map<string, number>
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

  const CONCURRENCY = 3;
  let i = 0;

  async function next(): Promise<void> {
    while (i < branches.length) {
      const branch = branches[i++]!;
      const knownNumber = prNumberCache?.get(branch);
      const info = await fetchPrInfo(repoPath, branch, knownNumber);
      result.set(branch, info);
      // If we entered backoff during this batch, fill remaining with null
      if (isInBackoff(repoPath)) {
        while (i < branches.length) {
          result.set(branches[i++]!, null);
        }
        return;
      }
    }
  }

  // Launch up to CONCURRENCY workers
  const workers: Promise<void>[] = [];
  for (let w = 0; w < Math.min(CONCURRENCY, branches.length); w++) {
    workers.push(next());
  }
  await Promise.all(workers);

  return result;
}

export function getPrStatusLabel(pr: PrInfo): { label: string; color: string } {
  const { state, reviewDecision, hasFeedback, isDraft, checksStatus } = pr;

  if (state === "MERGED") {
    if (checksStatus === "failing") {
      return { label: "Merged - Actions Failing", color: "magenta" };
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
