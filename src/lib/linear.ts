import http from "node:http";
import https from "node:https";
import { log } from "./logger.js";
import type { FetchResult, LinearInfo, PrInfo } from "./types.js";

const LINEAR_API_URL = process.env.AM_LINEAR_API_URL || "https://api.linear.app/graphql";

/** A non-2xx response. Carries the body so callers can surface Linear's own message. */
class HttpStatusError extends Error {
  constructor(readonly status: number, readonly body: string) {
    super(`HTTP ${status}: ${body.slice(0, 200)}`);
    this.name = "HttpStatusError";
  }
}

/** Pull the GraphQL error message out of a response body, if there is one. */
function graphqlMessage(body: string): string | null {
  try {
    const parsed = JSON.parse(body);
    const msg = parsed?.errors?.[0]?.message;
    return typeof msg === "string" ? msg : null;
  } catch {
    return null;
  }
}

function httpPost(
  url: string,
  headers: Record<string, string>,
  body: string,
  timeoutMs = 5000
): Promise<string> {
  return new Promise((resolve, reject) => {
    const parsed = new URL(url);
    const transport = parsed.protocol === "https:" ? https : http;
    const req = transport.request(
      {
        hostname: parsed.hostname,
        port: parsed.port || (parsed.protocol === "https:" ? 443 : 80),
        path: parsed.pathname + parsed.search,
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          ...headers,
        },
        timeout: timeoutMs,
      },
      (res) => {
        let data = "";
        res.on("data", (chunk: Buffer) => (data += chunk.toString()));
        res.on("end", () => {
          // A non-2xx body must not be parsed as a valid "no ticket" answer —
          // callers rely on a rejection to tell failure apart from absence.
          const status = res.statusCode ?? 0;
          if (status < 200 || status >= 300) {
            reject(new HttpStatusError(status, data));
            return;
          }
          resolve(data);
        });
      }
    );
    req.on("error", reject);
    req.on("timeout", () => {
      req.destroy();
      reject(new Error("Request timed out"));
    });
    req.write(body);
    req.end();
  });
}

const seenPrAttachments = new Set<string>();

/**
 * Fetch a branch's Linear ticket, distinguishing "no ticket for this branch" from
 * "the fetch failed". Callers that cache the result must only overwrite their cache
 * on `ok: true` — see `src/lib/integration-cache.ts`.
 */
export async function fetchLinearResult(
  branch: string,
  apiKey: string
): Promise<FetchResult<LinearInfo>> {
  const query = `
    query($branch: String!) {
      issueVcsBranchSearch(branchName: $branch) {
        identifier
        title
        url
        state { name color type }
        priorityLabel
        assignee { name }
        project { id name color url }
        attachments {
          nodes {
            url
            title
            sourceType
            metadata
          }
        }
      }
    }
  `;

  try {
    const raw = await httpPost(
      LINEAR_API_URL,
      { Authorization: apiKey },
      JSON.stringify({ query, variables: { branch } })
    );

    const json = JSON.parse(raw);
    // GraphQL reports auth/rate-limit/query problems in a 200 body; those are
    // failures, not "this branch has no ticket".
    if (Array.isArray(json?.errors) && json.errors.length > 0) {
      const msg = json.errors[0]?.message ?? "unknown GraphQL error";
      log("warn", "linear", `Linear API error for ${branch}: ${msg}`);
      return { ok: false, error: String(msg) };
    }
    const issue = json?.data?.issueVcsBranchSearch;
    if (!issue) return { ok: true, value: null };

    // Find first GitHub PR attachment for metadata inspection
    const attachments: Array<{ url: string; title: string; sourceType: string; metadata: Record<string, unknown> }> =
      issue.attachments?.nodes ?? [];
    const prAttachment = attachments.find(
      (a) => a.sourceType?.toLowerCase().includes("github") || a.url?.includes("/pull/")
    );
    if (prAttachment && !seenPrAttachments.has(prAttachment.url)) {
      seenPrAttachments.add(prAttachment.url);
      log("debug", "linear", `GitHub PR attachment for ${branch}: ${JSON.stringify(prAttachment)}`);
    }

    return {
      ok: true,
      value: {
        identifier: issue.identifier,
        title: issue.title,
        url: issue.url,
        state: issue.state,
        priorityLabel: issue.priorityLabel,
        assignee: issue.assignee?.name ?? null,
        project: issue.project ?? null,
        prAttachment: prAttachment
          ? { url: prAttachment.url, title: prAttachment.title, metadata: prAttachment.metadata }
          : null,
      },
    };
  } catch (err) {
    log("warn", "linear", `Failed to fetch Linear info for ${branch}: ${err}`);
    return { ok: false, error: String(err) };
  }
}

/**
 * Convenience wrapper for callers that treat a failed fetch the same as no ticket
 * (the one-shot CLI commands). Anything that caches should use `fetchLinearResult`.
 */
export async function fetchLinearInfo(
  branch: string,
  apiKey: string
): Promise<LinearInfo | null> {
  const result = await fetchLinearResult(branch, apiKey);
  return result.ok ? result.value : null;
}

export async function verifyLinearApiKey(apiKey: string): Promise<{ ok: boolean; name?: string; error?: string }> {
  try {
    const raw = await httpPost(
      LINEAR_API_URL,
      { Authorization: apiKey },
      JSON.stringify({ query: "{ viewer { name email } }" })
    );
    const json = JSON.parse(raw);
    if (json?.data?.viewer?.name) {
      return { ok: true, name: json.data.viewer.name };
    }
    const msg = json?.errors?.[0]?.message ?? "Invalid API key";
    return { ok: false, error: msg };
  } catch (err) {
    // Linear answers a bad key with a 401 whose body carries the real message —
    // prefer that over the bare status line.
    if (err instanceof HttpStatusError) {
      return { ok: false, error: graphqlMessage(err.body) ?? `HTTP ${err.status}` };
    }
    return { ok: false, error: String(err) };
  }
}

/**
 * Check if a Linear PR attachment's branch matches the given worktree branch.
 * Returns false if the attachment is for a different branch.
 */
export function linearAttachmentMatchesBranch(
  attachment: NonNullable<LinearInfo["prAttachment"]>,
  branch: string
): boolean {
  const prBranch = attachment.metadata?.branch as string | undefined;
  if (prBranch && prBranch !== branch) {
    log("debug", "linear", `PR attachment branch "${prBranch}" does not match worktree branch "${branch}", ignoring`);
    return false;
  }
  return true;
}

export function linearAttachmentToPrInfo(
  attachment: NonNullable<LinearInfo["prAttachment"]>
): PrInfo {
  const meta = attachment.metadata;
  const mergedAt = meta.mergedAt as string | null;
  const closedAt = meta.closedAt as string | null;

  let state = "OPEN";
  if (mergedAt) state = "MERGED";
  else if (closedAt) state = "CLOSED";

  return {
    number: (meta.number as number) ?? 0,
    title: attachment.title,
    url: attachment.url,
    state,
    isDraft: (meta.draft as boolean) ?? false,
    reviewDecision: "",
    hasFeedback: false,
    checksStatus: "none",
    activeCheckUrl: null,
    activeCheckName: null,
    checksWaiting: false,
  };
}

export function getLinearStatusColor(stateType: string): string {
  switch (stateType) {
    case "started":
      return "cyan";
    case "completed":
      return "green";
    case "canceled":
      return "red";
    case "backlog":
    case "triage":
    case "unstarted":
    default:
      return "gray";
  }
}
