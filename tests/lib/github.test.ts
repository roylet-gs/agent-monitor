import { describe, it, expect } from "vitest";
import { getPrStatusLabel, deriveChecksStatus } from "../../src/lib/github.js";
import type { PrInfo } from "../../src/lib/types.js";

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

  it("MERGED with failing checks", () => {
    const result = getPrStatusLabel(makePr({ state: "MERGED", checksStatus: "failing" }));
    expect(result.label).toBe("Merged - Actions Failing");
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
});
