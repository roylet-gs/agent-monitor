import http from "node:http";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const FIXTURES_DIR = join(__dirname, "fixtures");

function loadFixture(name: string): unknown {
  const raw = readFileSync(join(FIXTURES_DIR, name), "utf-8");
  return JSON.parse(raw);
}

// Default fixture state — can be overridden per-test via /mock/setup
let ghFixture: unknown = loadFixture("gh-pr-open.json");
let linearFixture: unknown = loadFixture("linear-issue.json");
let ghVersionResponse = "gh version 2.62.0 (2024-12-05)";
/** When set, the gh and Linear routes fail as if the network were down. */
let outage = false;
/**
 * Per-branch Linear overrides: branch name → the `issueVcsBranchSearch`
 * payload for that branch (`null` = no ticket). Branches not listed get the
 * fixture's issue.
 */
let linearByBranch: Record<string, unknown> = {};

interface GhFixture {
  number: number;
  title: string;
  url: string;
  state: string;
  isDraft?: boolean;
  reviewDecision?: string;
  mergeable?: string;
  headRefName?: string;
  statusCheckRollup?: Array<Record<string, unknown>>;
}

/** A `gh pr view --json` fixture as a GraphQL PullRequest node for `branch`. */
function toGraphqlPr(f: GhFixture, branch: string): unknown {
  return {
    number: f.number,
    title: f.title,
    url: f.url,
    state: f.state,
    isDraft: f.isDraft ?? false,
    reviewDecision: f.reviewDecision || null,
    mergeable: f.mergeable ?? "MERGEABLE",
    headRefName: f.headRefName ?? branch,
    isCrossRepository: false,
    commits: {
      nodes: [
        {
          commit: {
            statusCheckRollup: {
              contexts: {
                nodes: (f.statusCheckRollup ?? []).map((c) => ({ __typename: "CheckRun", ...c })),
              },
            },
          },
        },
      ],
    },
  };
}

/**
 * The Rust app batches branches into one request with aliases
 * (`b0: issueVcsBranchSearch(branchName: $b0)`). Answer each alias with the
 * fixture's issue. Unaliased (TS app) queries and fixtures without an issue
 * payload (e.g. an `errors` body) pass through unchanged.
 */
function aliasLinearFixture(
  fixture: unknown,
  query: string | undefined,
  variables: Record<string, unknown> | undefined,
): unknown {
  const aliases = [...(query ?? "").matchAll(/\b(b\d+)\s*:\s*issueVcsBranchSearch/g)].map((m) => m[1]!);
  const data = (fixture as { data?: Record<string, unknown> } | null)?.data;
  if (aliases.length === 0 || !data || !("issueVcsBranchSearch" in data)) return fixture;
  const answer = (alias: string): unknown => {
    const branch = variables?.[alias];
    if (typeof branch === "string" && branch in linearByBranch) return linearByBranch[branch];
    return data.issueVcsBranchSearch;
  };
  return { data: Object.fromEntries(aliases.map((a) => [a, answer(a)])) };
}

function readBody(req: http.IncomingMessage): Promise<string> {
  return new Promise((resolve) => {
    let body = "";
    req.on("data", (chunk: string) => (body += chunk));
    req.on("end", () => resolve(body));
  });
}

const server = http.createServer(async (req, res) => {
  const url = new URL(req.url ?? "/", `http://${req.headers.host}`);

  // CORS for test client
  res.setHeader("Access-Control-Allow-Origin", "*");
  res.setHeader("Access-Control-Allow-Methods", "GET, POST, DELETE, OPTIONS");
  res.setHeader("Access-Control-Allow-Headers", "Content-Type, Authorization");

  if (req.method === "OPTIONS") {
    res.writeHead(204);
    res.end();
    return;
  }

  // Health check
  if (url.pathname === "/health" && req.method === "GET") {
    res.writeHead(200, { "Content-Type": "application/json" });
    res.end(JSON.stringify({ status: "ok" }));
    return;
  }

  // Fake gh CLI endpoint
  if (url.pathname === "/gh" && req.method === "POST") {
    const body = await readBody(req);
    let parsed: { args?: string[]; cwd?: string };
    try {
      parsed = JSON.parse(body);
    } catch {
      parsed = {};
    }

    const args = parsed.args ?? [];
    const argsStr = args.join(" ");

    // Handle gh --version
    if (argsStr.includes("--version")) {
      res.writeHead(200, { "Content-Type": "text/plain" });
      res.end(ghVersionResponse);
      return;
    }

    // Handle the Rust app's batched PR lookup:
    //   gh api graphql -F owner={owner} -F name={repo} -f b0=<branch> -f b1=<branch> … -f query=…
    // Answers every aliased branch with the current gh fixture, reshaped into the
    // GraphQL `pullRequests` connection the query asks for.
    if (args[0] === "api" && args[1] === "graphql") {
      if (outage) {
        res.writeHead(503, { "Content-Type": "text/plain" });
        res.end("simulated outage");
        return;
      }
      const repository: Record<string, { nodes: unknown[] }> = {};
      for (let i = 0; i < args.length - 1; i++) {
        if (args[i] !== "-f" && args[i] !== "-F") continue;
        const m = /^(b\d+)=([\s\S]*)$/.exec(args[i + 1] ?? "");
        if (!m) continue;
        repository[m[1]!] = {
          nodes: ghFixture === null ? [] : [toGraphqlPr(ghFixture as GhFixture, m[2]!)],
        };
      }
      res.writeHead(200, { "Content-Type": "application/json" });
      res.end(JSON.stringify({ data: { repository } }));
      return;
    }

    // Handle gh pr view
    if (argsStr.includes("pr") && argsStr.includes("view")) {
      if (outage) {
        res.writeHead(503, { "Content-Type": "text/plain" });
        res.end("simulated outage");
        return;
      }
      if (ghFixture === null) {
        res.writeHead(404, { "Content-Type": "text/plain" });
        res.end("no pull requests found");
        return;
      }
      res.writeHead(200, { "Content-Type": "application/json" });
      res.end(JSON.stringify(ghFixture));
      return;
    }

    // Default: no data
    res.writeHead(200, { "Content-Type": "text/plain" });
    res.end("");
    return;
  }

  // Fake Linear GraphQL endpoint
  if (url.pathname === "/linear" && req.method === "POST") {
    if (outage) {
      res.writeHead(503, { "Content-Type": "application/json" });
      res.end(JSON.stringify({ message: "simulated outage" }));
      return;
    }
    const body = await readBody(req);
    let parsed: { query?: string; variables?: Record<string, unknown> };
    try {
      parsed = JSON.parse(body);
    } catch {
      parsed = {};
    }

    // Handle viewer query (API key verification)
    if (parsed.query?.includes("viewer")) {
      res.writeHead(200, { "Content-Type": "application/json" });
      res.end(JSON.stringify({
        data: { viewer: { name: "Test User", email: "test@test.com" } },
      }));
      return;
    }

    // Handle issue search
    res.writeHead(200, { "Content-Type": "application/json" });
    res.end(JSON.stringify(aliasLinearFixture(linearFixture, parsed.query, parsed.variables)));
    return;
  }

  // Per-test fixture setup
  if (url.pathname === "/mock/setup" && req.method === "POST") {
    const body = await readBody(req);
    try {
      const config = JSON.parse(body);
      if (config.gh !== undefined) {
        ghFixture = config.gh === null ? null : config.gh;
      }
      if (config.linear !== undefined) {
        linearFixture = config.linear;
      }
      if (config.ghVersion !== undefined) {
        ghVersionResponse = config.ghVersion;
      }
      if (config.outage !== undefined) {
        outage = Boolean(config.outage);
      }
      if (config.linearByBranch !== undefined) {
        linearByBranch = config.linearByBranch ?? {};
      }
      res.writeHead(200, { "Content-Type": "application/json" });
      res.end(JSON.stringify({ status: "configured" }));
    } catch (err) {
      res.writeHead(400, { "Content-Type": "application/json" });
      res.end(JSON.stringify({ error: String(err) }));
    }
    return;
  }

  // Reset fixtures to defaults
  if (url.pathname === "/mock/reset" && req.method === "DELETE") {
    ghFixture = loadFixture("gh-pr-open.json");
    linearFixture = loadFixture("linear-issue.json");
    ghVersionResponse = "gh version 2.62.0 (2024-12-05)";
    outage = false;
    linearByBranch = {};
    res.writeHead(200, { "Content-Type": "application/json" });
    res.end(JSON.stringify({ status: "reset" }));
    return;
  }

  res.writeHead(404);
  res.end("Not Found");
});

const PORT = Number(process.env.MOCK_API_PORT) || 4100;
server.listen(PORT, () => {
  console.log(`Mock API listening on port ${PORT}`);
});
