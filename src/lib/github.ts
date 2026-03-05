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
  latestReviews: Array<{ state: string }>;
  statusCheckRollup: Array<{ status: string; conclusion: string }>;
  comments: Array<unknown>;
}

const GH_PR_FIELDS = "number,title,url,state,isDraft,reviewDecision,latestReviews,statusCheckRollup,comments";

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

// --- Exponential backoff state ---
let backoffUntil = 0;
let backoffMs = 0;
const BACKOFF_INITIAL_MS = 5_000;
const BACKOFF_MAX_MS = 300_000; // 5 minutes

function isInBackoff(): boolean {
  if (backoffUntil === 0) return false;
  if (Date.now() >= backoffUntil) {
    // Backoff period expired, allow retry
    return false;
  }
  return true;
}

function onGhSuccess(): void {
  if (backoffMs > 0) {
    log("info", "github", "GitHub API recovered, resetting backoff");
  }
  backoffUntil = 0;
  backoffMs = 0;
}

function onGhFailure(err: unknown): void {
  backoffMs = backoffMs === 0 ? BACKOFF_INITIAL_MS : Math.min(backoffMs * 2, BACKOFF_MAX_MS);
  backoffUntil = Date.now() + backoffMs;
  log("warn", "github", `GitHub API error, backing off for ${backoffMs / 1000}s: ${err}`);
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
  const hasReviewFeedback = (pr.latestReviews ?? []).some(
    (r) => r.state === "COMMENTED" || r.state === "CHANGES_REQUESTED"
  );
  const hasDraftComments = pr.isDraft && (pr.comments ?? []).length > 0;
  const hasFeedback = hasReviewFeedback || hasDraftComments;

  return {
    number: pr.number,
    title: pr.title,
    url: pr.url,
    state: pr.state,
    isDraft: pr.isDraft,
    reviewDecision: pr.reviewDecision ?? "",
    hasFeedback,
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
  if (isInBackoff()) {
    log("debug", "github", `Skipping PR fetch for ${branch} (in backoff)`);
    return null;
  }

  try {
    const target = prNumber != null ? String(prNumber) : branch;
    const stdout = await execGh(
      ["pr", "view", target, "--json", GH_PR_FIELDS],
      repoPath
    );

    const pr: GhPrResult = JSON.parse(stdout);
    onGhSuccess();
    return ghResultToPrInfo(pr);
  } catch (err) {
    const msg = String(err);
    // "no pull requests found" is not an API error, just means no PR exists
    if (msg.includes("no pull requests found") || msg.includes("Could not resolve")) {
      onGhSuccess();
      return null;
    }
    onGhFailure(err);
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

  if (isInBackoff()) {
    log("debug", "github", `Skipping all PR fetches for ${repoPath} (in backoff)`);
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
      if (isInBackoff()) {
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
