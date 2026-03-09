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

    // Handle gh pr view
    if (argsStr.includes("pr") && argsStr.includes("view")) {
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
    const body = await readBody(req);
    let parsed: { query?: string };
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
    res.end(JSON.stringify(linearFixture));
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
