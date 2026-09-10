import { describe, it, expect, vi, afterEach } from "vitest";
import http from "node:http";
import type { AddressInfo } from "node:net";
import { linearAttachmentMatchesBranch, linearAttachmentToPrInfo } from "../../src/lib/linear.js";

function makeAttachment(branch?: string) {
  return {
    url: "https://github.com/test/test/pull/1",
    title: "Test PR",
    metadata: {
      number: 1,
      draft: false,
      mergedAt: null,
      closedAt: null,
      ...(branch != null ? { branch } : {}),
    } as Record<string, unknown>,
  };
}

describe("linearAttachmentMatchesBranch", () => {
  it("returns true when attachment branch matches worktree branch", () => {
    const attachment = makeAttachment("feature/foo");
    expect(linearAttachmentMatchesBranch(attachment, "feature/foo")).toBe(true);
  });

  it("returns false when attachment branch differs from worktree branch", () => {
    const attachment = makeAttachment("feature/foo-other");
    expect(linearAttachmentMatchesBranch(attachment, "feature/foo")).toBe(false);
  });

  it("returns true when attachment has no branch metadata", () => {
    const attachment = makeAttachment();
    expect(linearAttachmentMatchesBranch(attachment, "feature/foo")).toBe(true);
  });

  it("rejects partial branch name matches", () => {
    const attachment = makeAttachment("feature/connect-86-asset-features-refactor");
    expect(linearAttachmentMatchesBranch(attachment, "feature/connect-86-network-graph-map")).toBe(false);
  });
});

describe("linearAttachmentToPrInfo", () => {
  it("converts an open PR attachment", () => {
    const info = linearAttachmentToPrInfo(makeAttachment("feature/foo"));
    expect(info.state).toBe("OPEN");
    expect(info.number).toBe(1);
    expect(info.title).toBe("Test PR");
  });

  it("detects merged state from mergedAt", () => {
    const attachment = makeAttachment("main");
    attachment.metadata.mergedAt = "2026-01-01T00:00:00Z";
    const info = linearAttachmentToPrInfo(attachment);
    expect(info.state).toBe("MERGED");
  });

  it("detects closed state from closedAt", () => {
    const attachment = makeAttachment("main");
    attachment.metadata.closedAt = "2026-01-01T00:00:00Z";
    const info = linearAttachmentToPrInfo(attachment);
    expect(info.state).toBe("CLOSED");
  });
});

describe("fetchLinearInfo", () => {
  let server: http.Server | null = null;

  afterEach(async () => {
    if (server) {
      await new Promise<void>((resolve) => server!.close(() => resolve()));
      server = null;
    }
    delete process.env.AM_LINEAR_API_URL;
    vi.resetModules();
  });

  async function serveIssue(issue: unknown) {
    server = http.createServer((_req, res) => {
      res.setHeader("Content-Type", "application/json");
      res.end(JSON.stringify({ data: { issueVcsBranchSearch: issue } }));
    });
    await new Promise<void>((resolve) => server!.listen(0, resolve));
    const { port } = server!.address() as AddressInfo;
    process.env.AM_LINEAR_API_URL = `http://127.0.0.1:${port}/graphql`;
    vi.resetModules();
    const { fetchLinearInfo } = await import("../../src/lib/linear.js");
    return fetchLinearInfo;
  }

  const baseIssue = {
    identifier: "ENG-1",
    title: "Ticket",
    url: "https://linear.app/team/issue/ENG-1",
    state: { name: "In Progress", color: "#0ea5e9", type: "started" },
    priorityLabel: "High",
    assignee: null,
    attachments: { nodes: [] },
  };

  it("maps the issue's project", async () => {
    const project = { id: "proj-1", name: "Dashboard Revamp", color: "#5e6ad2", url: "https://linear.app/team/project/proj-1" };
    const fetchLinearInfo = await serveIssue({ ...baseIssue, project });
    const info = await fetchLinearInfo("feature/x", "key");
    expect(info?.identifier).toBe("ENG-1");
    expect(info?.project).toEqual(project);
  });

  it("maps project to null when the issue has none", async () => {
    const fetchLinearInfo = await serveIssue(baseIssue);
    const info = await fetchLinearInfo("feature/x", "key");
    expect(info?.identifier).toBe("ENG-1");
    expect(info?.project).toBeNull();
  });
});

/**
 * A cached ticket must only be cleared by an authoritative "there is no ticket".
 * Everything else — HTTP errors, GraphQL errors, a dead socket, junk bodies — has to
 * report failure so the caller keeps what it already had.
 */
describe("fetchLinearResult", () => {
  let server: http.Server | null = null;

  afterEach(async () => {
    if (server) {
      await new Promise<void>((resolve) => server!.close(() => resolve()));
      server = null;
    }
    delete process.env.AM_LINEAR_API_URL;
    vi.resetModules();
  });

  async function serveRaw(handler: http.RequestListener) {
    server = http.createServer(handler);
    await new Promise<void>((resolve) => server!.listen(0, resolve));
    const { port } = server!.address() as AddressInfo;
    process.env.AM_LINEAR_API_URL = `http://127.0.0.1:${port}/graphql`;
    vi.resetModules();
    return (await import("../../src/lib/linear.js")).fetchLinearResult;
  }

  function serveJson(status: number, body: unknown): http.RequestListener {
    return (_req, res) => {
      res.writeHead(status, { "Content-Type": "application/json" });
      res.end(JSON.stringify(body));
    };
  }

  const issue = {
    identifier: "ENG-1",
    title: "Ticket",
    url: "https://linear.app/team/issue/ENG-1",
    state: { name: "In Progress", color: "#0ea5e9", type: "started" },
    priorityLabel: "High",
    assignee: null,
    attachments: { nodes: [] },
  };

  it("reports success with the ticket when one exists", async () => {
    const fetchLinearResult = await serveRaw(
      serveJson(200, { data: { issueVcsBranchSearch: issue } })
    );
    const result = await fetchLinearResult("feature/x", "key");
    expect(result).toMatchObject({ ok: true });
    expect(result.ok && result.value?.identifier).toBe("ENG-1");
  });

  it("reports success with a null value when the branch genuinely has no ticket", async () => {
    const fetchLinearResult = await serveRaw(
      serveJson(200, { data: { issueVcsBranchSearch: null } })
    );
    expect(await fetchLinearResult("feature/x", "key")).toEqual({ ok: true, value: null });
  });

  it("reports failure on a 500, rather than pretending there is no ticket", async () => {
    const fetchLinearResult = await serveRaw(serveJson(500, { message: "boom" }));
    const result = await fetchLinearResult("feature/x", "key");
    expect(result.ok).toBe(false);
  });

  it("reports failure on a 401", async () => {
    const fetchLinearResult = await serveRaw(serveJson(401, { message: "unauthorized" }));
    const result = await fetchLinearResult("feature/x", "key");
    expect(result.ok).toBe(false);
  });

  it("reports failure when a 200 body carries GraphQL errors", async () => {
    const fetchLinearResult = await serveRaw(
      serveJson(200, { errors: [{ message: "authentication failed" }] })
    );
    const result = await fetchLinearResult("feature/x", "key");
    expect(result.ok).toBe(false);
    expect(result.ok === false && result.error).toContain("authentication failed");
  });

  it("reports failure on an unparseable body", async () => {
    const fetchLinearResult = await serveRaw((_req, res) => res.end("not json"));
    const result = await fetchLinearResult("feature/x", "key");
    expect(result.ok).toBe(false);
  });

  it("reports failure when the host is unreachable", async () => {
    // Bind to grab a free port, then close it so the connection is refused.
    const dead = http.createServer(() => {});
    await new Promise<void>((resolve) => dead.listen(0, resolve));
    const { port } = dead.address() as AddressInfo;
    await new Promise<void>((resolve) => dead.close(() => resolve()));

    process.env.AM_LINEAR_API_URL = `http://127.0.0.1:${port}/graphql`;
    vi.resetModules();
    const { fetchLinearResult } = await import("../../src/lib/linear.js");
    const result = await fetchLinearResult("feature/x", "key");
    expect(result.ok).toBe(false);
  });

  it("surfaces Linear's own message when the key is rejected", async () => {
    await serveRaw(serveJson(401, { errors: [{ message: "Authentication failed" }] }));
    const { verifyLinearApiKey } = await import("../../src/lib/linear.js");
    expect(await verifyLinearApiKey("bad-key")).toEqual({
      ok: false,
      error: "Authentication failed",
    });
  });

  it("falls back to the status line when a rejection has no GraphQL message", async () => {
    await serveRaw((_req, res) => {
      res.writeHead(502, { "Content-Type": "text/plain" });
      res.end("bad gateway");
    });
    const { verifyLinearApiKey } = await import("../../src/lib/linear.js");
    expect(await verifyLinearApiKey("key")).toEqual({ ok: false, error: "HTTP 502" });
  });

  it("keeps fetchLinearInfo lossy for the one-shot CLI callers", async () => {
    const fetchLinearResultForServer = await serveRaw(serveJson(500, { message: "boom" }));
    expect((await fetchLinearResultForServer("feature/x", "key")).ok).toBe(false);
    const { fetchLinearInfo } = await import("../../src/lib/linear.js");
    expect(await fetchLinearInfo("feature/x", "key")).toBeNull();
  });
});
