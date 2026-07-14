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
