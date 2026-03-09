import http from "node:http";
import https from "node:https";
import { log } from "./logger.js";
import type { LinearInfo, PrInfo } from "./types.js";

const LINEAR_API_URL = process.env.AM_LINEAR_API_URL || "https://api.linear.app/graphql";

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
        res.on("end", () => resolve(data));
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

export async function fetchLinearInfo(
  branch: string,
  apiKey: string
): Promise<LinearInfo | null> {
  const query = `
    query($branch: String!) {
      issueVcsBranchSearch(branchName: $branch) {
        identifier
        title
        url
        state { name color type }
        priorityLabel
        assignee { name }
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
    const issue = json?.data?.issueVcsBranchSearch;
    if (!issue) return null;

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
      identifier: issue.identifier,
      title: issue.title,
      url: issue.url,
      state: issue.state,
      priorityLabel: issue.priorityLabel,
      assignee: issue.assignee?.name ?? null,
      prAttachment: prAttachment
        ? { url: prAttachment.url, title: prAttachment.title, metadata: prAttachment.metadata }
        : null,
    };
  } catch (err) {
    log("debug", "linear", `Failed to fetch Linear info for ${branch}: ${err}`);
    return null;
  }
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
    return { ok: false, error: String(err) };
  }
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
