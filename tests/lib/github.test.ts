import { describe, it, expect, vi, beforeEach } from "vitest";
import { getPrStatusLabel, deriveChecksStatus, shouldSkipPrFetch, getOwnRepoSlug, ghBranchExists } from "../../src/lib/github.js";
import type { PrInfo } from "../../src/lib/types.js";

const mockExecFile = vi.fn();
vi.mock("child_process", async (importOriginal) => {
  const actual = await importOriginal<typeof import("child_process")>();
  return {
    ...actual,
    execFile: (...args: unknown[]) => mockExecFile(...args),
  };
});

function makePr(overrides: Partial<PrInfo> = {}): PrInfo {
  return {
    number: 1,
    title: "Test PR",
    url: "https://github.com/test/test/pull/1",
    state: "OPEN",
    isDraft: false,
    reviewDecision: "",
    hasFeedback: false,
    checksStatus: "none",
    activeCheckUrl: null,
    activeCheckName: null,
    checksWaiting: false,
    ...overrides,
  };
}

describe("deriveChecksStatus", () => {
  it("returns 'none' for empty checks", () => {
    expect(deriveChecksStatus([])).toBe("none");
  });

  it("returns 'none' for null/undefined checks", () => {
    expect(deriveChecksStatus(null as any)).toBe("none");
  });

  it("returns 'passing' when all completed successfully", () => {
    expect(
      deriveChecksStatus([
        { status: "COMPLETED", conclusion: "SUCCESS" },
        { status: "COMPLETED", conclusion: "SUCCESS" },
      ])
    ).toBe("passing");
  });

  it("returns 'failing' when any check failed", () => {
    expect(
      deriveChecksStatus([
        { status: "COMPLETED", conclusion: "SUCCESS" },
        { status: "COMPLETED", conclusion: "FAILURE" },
      ])
    ).toBe("failing");
  });

  it("returns 'failing' for ERROR conclusion", () => {
    expect(
      deriveChecksStatus([{ status: "COMPLETED", conclusion: "ERROR" }])
    ).toBe("failing");
  });

  it("returns 'failing' for CANCELLED conclusion", () => {
    expect(
      deriveChecksStatus([{ status: "COMPLETED", conclusion: "CANCELLED" }])
    ).toBe("failing");
  });

  it("returns 'pending' when check is in progress", () => {
    expect(
      deriveChecksStatus([
        { status: "IN_PROGRESS", conclusion: "" },
        { status: "COMPLETED", conclusion: "SUCCESS" },
      ])
    ).toBe("pending");
  });

  it("failure takes priority over pending", () => {
    expect(
      deriveChecksStatus([
        { status: "IN_PROGRESS", conclusion: "" },
        { status: "COMPLETED", conclusion: "FAILURE" },
      ])
    ).toBe("failing");
  });
});

describe("getPrStatusLabel", () => {
  it("MERGED -> Merged (magenta)", () => {
    const result = getPrStatusLabel(makePr({ state: "MERGED" }));
    expect(result).toEqual({ label: "Merged", color: "magenta" });
  });

  it("MERGED with failing checks (red)", () => {
    const result = getPrStatusLabel(makePr({ state: "MERGED", checksStatus: "failing" }));
    expect(result).toEqual({ label: "Merged - Actions Failing", color: "red" });
  });

  it("MERGED with pending checks", () => {
    const result = getPrStatusLabel(makePr({ state: "MERGED", checksStatus: "pending" }));
    expect(result.label).toBe("Merged - Actions Running");
  });

  it("CLOSED -> Closed (red)", () => {
    const result = getPrStatusLabel(makePr({ state: "CLOSED" }));
    expect(result).toEqual({ label: "Closed", color: "red" });
  });

  it("CHANGES_REQUESTED -> Changes Requested (red)", () => {
    const result = getPrStatusLabel(makePr({ reviewDecision: "CHANGES_REQUESTED" }));
    expect(result).toEqual({ label: "Changes Requested", color: "red" });
  });

  it("APPROVED -> Approved (green)", () => {
    const result = getPrStatusLabel(makePr({ reviewDecision: "APPROVED" }));
    expect(result).toEqual({ label: "Approved", color: "green" });
  });

  it("APPROVED with failing checks", () => {
    const result = getPrStatusLabel(
      makePr({ reviewDecision: "APPROVED", checksStatus: "failing" })
    );
    expect(result.label).toBe("Approved - Checks Failing");
  });

  it("APPROVED with pending checks", () => {
    const result = getPrStatusLabel(
      makePr({ reviewDecision: "APPROVED", checksStatus: "pending" })
    );
    expect(result.label).toBe("Approved - Checks Running");
  });

  it("Draft PR -> Draft (gray)", () => {
    const result = getPrStatusLabel(makePr({ isDraft: true }));
    expect(result).toEqual({ label: "Draft", color: "gray" });
  });

  it("Draft PR with feedback", () => {
    const result = getPrStatusLabel(makePr({ isDraft: true, hasFeedback: true }));
    expect(result.label).toBe("Draft - Feedback");
  });

  it("In Review (default open PR)", () => {
    const result = getPrStatusLabel(makePr());
    expect(result).toEqual({ label: "In Review", color: "cyan" });
  });

  it("In Review with feedback", () => {
    const result = getPrStatusLabel(makePr({ hasFeedback: true }));
    expect(result.label).toBe("In Review - Feedback");
  });

  it("In Review with failing checks", () => {
    const result = getPrStatusLabel(makePr({ checksStatus: "failing" }));
    expect(result.label).toBe("In Review - Checks Failing");
  });

  it("In Review with pending checks", () => {
    const result = getPrStatusLabel(makePr({ checksStatus: "pending" }));
    expect(result.label).toBe("In Review - Checks Running");
  });

  it("MERGED with checksWaiting -> Awaiting Approval (yellow)", () => {
    const result = getPrStatusLabel(
      makePr({ state: "MERGED", checksStatus: "pending", checksWaiting: true })
    );
    expect(result).toEqual({ label: "Merged - Awaiting Approval", color: "yellow" });
  });

  it("MERGED failing takes priority over checksWaiting", () => {
    const result = getPrStatusLabel(
      makePr({ state: "MERGED", checksStatus: "failing", checksWaiting: true })
    );
    expect(result.label).toBe("Merged - Actions Failing");
  });
});

describe("shouldSkipPrFetch", () => {
  it("returns false for null cache", () => {
    expect(shouldSkipPrFetch(null)).toBe(false);
  });

  it("returns false for open PRs", () => {
    expect(shouldSkipPrFetch(makePr({ state: "OPEN" }))).toBe(false);
  });

  it("returns true for closed PRs", () => {
    expect(shouldSkipPrFetch(makePr({ state: "CLOSED" }))).toBe(true);
  });

  it("returns true for merged PR with passing checks", () => {
    expect(shouldSkipPrFetch(makePr({ state: "MERGED", checksStatus: "passing" }))).toBe(true);
  });

  it("returns true for merged PR with no checks", () => {
    expect(shouldSkipPrFetch(makePr({ state: "MERGED", checksStatus: "none" }))).toBe(true);
  });

  it("returns false for merged PR with pending checks (deployment active)", () => {
    expect(shouldSkipPrFetch(makePr({ state: "MERGED", checksStatus: "pending" }))).toBe(false);
  });

  it("returns false for merged PR with failing checks", () => {
    expect(shouldSkipPrFetch(makePr({ state: "MERGED", checksStatus: "failing" }))).toBe(false);
  });
});

describe("getOwnRepoSlug", () => {
  it("derives owner/name from the app's package.json repository url", () => {
    expect(getOwnRepoSlug()).toBe("roylet-gs/agent-monitor");
  });
});

describe("ghBranchExists", () => {
  type ExecFileCallback = (err: Error | null, stdout: string, stderr: string) => void;

  const ghReturns = (stdout: string) => {
    mockExecFile.mockImplementationOnce(
      (_cmd: string, _args: string[], _opts: unknown, cb: ExecFileCallback) => {
        cb(null, stdout, "");
      }
    );
  };

  const ghFails = (stderr: string) => {
    mockExecFile.mockImplementationOnce(
      (_cmd: string, _args: string[], _opts: unknown, cb: ExecFileCallback) => {
        cb(new Error("gh exited 1"), "", stderr);
      }
    );
  };

  beforeEach(() => {
    mockExecFile.mockReset();
  });

  it("returns true when the API returns the exact ref", async () => {
    ghReturns("refs/heads/feature/connect-848\n");
    expect(await ghBranchExists("/repo", "feature/connect-848")).toBe(true);
    expect(mockExecFile).toHaveBeenCalledWith(
      "gh",
      ["api", "repos/{owner}/{repo}/git/ref/heads/feature/connect-848", "--jq", ".ref"],
      expect.objectContaining({ cwd: "/repo" }),
      expect.any(Function)
    );
  });

  it("returns false on HTTP 404 (authoritative no)", async () => {
    ghFails("gh: Not Found (HTTP 404)");
    expect(await ghBranchExists("/repo", "feature/nope")).toBe(false);
  });

  it("returns null on other failures (offline, no gh, non-GitHub origin)", async () => {
    ghFails("error connecting to api.github.com");
    expect(await ghBranchExists("/repo", "feature/x")).toBe(null);
  });

  it("returns false when the API answers with a different ref", async () => {
    ghReturns("null\n");
    expect(await ghBranchExists("/repo", "feature/x")).toBe(false);
  });
});
