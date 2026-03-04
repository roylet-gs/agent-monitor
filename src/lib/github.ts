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
  headRefName: string;
}

function execGh(args: string[], cwd: string): Promise<string> {
  return new Promise((resolve, reject) => {
    execFile("gh", args, { cwd, timeout: 5000 }, (err, stdout) => {
      if (err) {
        reject(err);
      } else {
        resolve(stdout);
      }
    });
  });
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

export async function fetchPrInfo(
  repoPath: string,
  branch: string
): Promise<PrInfo | null> {
  try {
    const stdout = await execGh(
      [
        "pr",
        "list",
        "--head",
        branch,
        "--state",
        "all",
        "--json",
        "number,title,url,state,isDraft,reviewDecision,latestReviews,statusCheckRollup,comments",
        "--limit",
        "1",
      ],
      repoPath
    );

    const results: GhPrResult[] = JSON.parse(stdout);
    if (!results || results.length === 0) return null;

    return ghResultToPrInfo(results[0]!);
  } catch (err) {
    log("debug", "github", `Failed to fetch PR info for ${branch}: ${err}`);
    return null;
  }
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

export async function fetchAllPrInfo(
  repoPath: string,
  branches: string[]
): Promise<Map<string, PrInfo | null>> {
  const result = new Map<string, PrInfo | null>();
  if (branches.length === 0) return result;

  // Initialize all branches to null
  for (const b of branches) {
    result.set(b, null);
  }

  try {
    const stdout = await new Promise<string>((resolve, reject) => {
      execFile(
        "gh",
        [
          "pr", "list",
          "--state", "all",
          "--json", "number,title,url,state,isDraft,reviewDecision,latestReviews,statusCheckRollup,comments,headRefName",
          "--limit", "100",
        ],
        { cwd: repoPath, timeout: 10000 },
        (err, out) => {
          if (err) reject(err);
          else resolve(out);
        }
      );
    });

    const prs: GhPrResult[] = JSON.parse(stdout);
    const branchSet = new Set(branches);

    for (const pr of prs) {
      if (branchSet.has(pr.headRefName) && result.get(pr.headRefName) === null) {
        result.set(pr.headRefName, ghResultToPrInfo(pr));
      }
    }
  } catch (err) {
    log("debug", "github", `Failed to batch-fetch PR info for ${repoPath}: ${err}`);
  }

  return result;
}

export function getPrStatusLabel(pr: PrInfo): { label: string; color: string } {
  const { state, reviewDecision, hasFeedback, isDraft, checksStatus } = pr;

  if (state === "MERGED") {
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
