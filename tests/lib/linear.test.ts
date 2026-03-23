import { describe, it, expect } from "vitest";
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
