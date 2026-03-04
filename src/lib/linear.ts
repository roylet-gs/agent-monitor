import https from "node:https";
import { log } from "./logger.js";
import type { LinearInfo } from "./types.js";

function httpsPost(
  url: string,
  headers: Record<string, string>,
  body: string,
  timeoutMs = 5000
): Promise<string> {
  return new Promise((resolve, reject) => {
    const parsed = new URL(url);
    const req = https.request(
      {
        hostname: parsed.hostname,
        path: parsed.pathname,
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
      }
    }
  `;

  try {
    const raw = await httpsPost(
      "https://api.linear.app/graphql",
      { Authorization: apiKey },
      JSON.stringify({ query, variables: { branch } })
    );

    const json = JSON.parse(raw);
    const issue = json?.data?.issueVcsBranchSearch;
    if (!issue) return null;

    return {
      identifier: issue.identifier,
      title: issue.title,
      url: issue.url,
      state: issue.state,
      priorityLabel: issue.priorityLabel,
      assignee: issue.assignee?.name ?? null,
    };
  } catch (err) {
    log("debug", "linear", `Failed to fetch Linear info for ${branch}: ${err}`);
    return null;
  }
}

export async function verifyLinearApiKey(apiKey: string): Promise<{ ok: boolean; name?: string; error?: string }> {
  try {
    const raw = await httpsPost(
      "https://api.linear.app/graphql",
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
