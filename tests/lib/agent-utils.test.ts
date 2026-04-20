import { describe, it, expect } from "vitest";
import { isEffectivelyOpen, getDisplayStatus, getDisplayStatusStandalone, normalizeSummary } from "../../src/lib/agent-utils.js";
import type { StandaloneSession } from "../../src/lib/types.js";
import type { AgentStatus } from "../../src/lib/types.js";

// SQLite datetime('now') format: "YYYY-MM-DD HH:MM:SS" (no timezone suffix)
function sqliteNow(): string {
  return new Date().toISOString().replace("T", " ").replace(/\.\d{3}Z$/, "");
}

function sqliteDate(date: Date): string {
  return date.toISOString().replace("T", " ").replace(/\.\d{3}Z$/, "");
}

function makeStatus(overrides: Partial<AgentStatus> = {}): AgentStatus {
  return {
    worktree_id: "wt-1",
    status: "idle",
    last_response: null,
    transcript_summary: null,
    session_id: null,
    is_open: 0,
    updated_at: sqliteNow(),
    ...overrides,
  };
}

describe("isEffectivelyOpen", () => {
  it("returns false for null", () => {
    expect(isEffectivelyOpen(null)).toBe(false);
  });

  it("returns false for undefined", () => {
    expect(isEffectivelyOpen(undefined)).toBe(false);
  });

  it("returns false when is_open is 0", () => {
    expect(isEffectivelyOpen(makeStatus({ is_open: 0 }))).toBe(false);
  });

  it("returns true when is_open is 1 and status is executing", () => {
    expect(isEffectivelyOpen(makeStatus({ is_open: 1, status: "executing" }))).toBe(true);
  });

  it("returns true when is_open is 1 and status is planning", () => {
    expect(isEffectivelyOpen(makeStatus({ is_open: 1, status: "planning" }))).toBe(true);
  });

  it("returns true when is_open is 1 and status is waiting", () => {
    expect(isEffectivelyOpen(makeStatus({ is_open: 1, status: "waiting" }))).toBe(true);
  });

  it("returns true when idle and recently updated", () => {
    expect(isEffectivelyOpen(makeStatus({
      is_open: 1,
      status: "idle",
      updated_at: sqliteNow(),
    }))).toBe(true);
  });

  it("returns false when idle and stale (>10min)", () => {
    const staleTime = sqliteDate(new Date(Date.now() - 11 * 60 * 1000));
    expect(isEffectivelyOpen(makeStatus({
      is_open: 1,
      status: "idle",
      updated_at: staleTime,
    }))).toBe(false);
  });

  it("returns true when executing even if stale", () => {
    const staleTime = sqliteDate(new Date(Date.now() - 30 * 60 * 1000));
    expect(isEffectivelyOpen(makeStatus({
      is_open: 1,
      status: "executing",
      updated_at: staleTime,
    }))).toBe(true);
  });
});

describe("getDisplayStatus", () => {
  it("returns 'none' for null", () => {
    expect(getDisplayStatus(null)).toBe("none");
  });

  it("returns 'none' for undefined", () => {
    expect(getDisplayStatus(undefined)).toBe("none");
  });

  it("returns actual status when recently updated", () => {
    expect(getDisplayStatus(makeStatus({ status: "executing" }))).toBe("executing");
  });

  it("returns 'waiting' for stale executing (>5 min)", () => {
    const staleTime = sqliteDate(new Date(Date.now() - 6 * 60 * 1000));
    expect(getDisplayStatus(makeStatus({ status: "executing", updated_at: staleTime }))).toBe("waiting");
  });

  it("returns 'waiting' for stale planning (>5 min)", () => {
    const staleTime = sqliteDate(new Date(Date.now() - 6 * 60 * 1000));
    expect(getDisplayStatus(makeStatus({ status: "planning", updated_at: staleTime }))).toBe("waiting");
  });

  it("returns executing when updated 4 min ago (not yet stale)", () => {
    const recentTime = sqliteDate(new Date(Date.now() - 4 * 60 * 1000));
    expect(getDisplayStatus(makeStatus({ status: "executing", updated_at: recentTime }))).toBe("executing");
  });

  it("returns planning when updated 4 min ago (not yet stale)", () => {
    const recentTime = sqliteDate(new Date(Date.now() - 4 * 60 * 1000));
    expect(getDisplayStatus(makeStatus({ status: "planning", updated_at: recentTime }))).toBe("planning");
  });

  it("returns 'idle' as-is regardless of staleness", () => {
    const staleTime = sqliteDate(new Date(Date.now() - 10 * 60 * 1000));
    expect(getDisplayStatus(makeStatus({ status: "idle", updated_at: staleTime }))).toBe("idle");
  });

  it("returns 'waiting' as-is regardless of staleness", () => {
    const staleTime = sqliteDate(new Date(Date.now() - 10 * 60 * 1000));
    expect(getDisplayStatus(makeStatus({ status: "waiting", updated_at: staleTime }))).toBe("waiting");
  });

  it("returns 'done' as-is regardless of staleness", () => {
    const staleTime = sqliteDate(new Date(Date.now() - 10 * 60 * 1000));
    expect(getDisplayStatus(makeStatus({ status: "done", updated_at: staleTime }))).toBe("done");
  });
});

describe("getDisplayStatusStandalone", () => {
  function makeSession(overrides: Partial<StandaloneSession> = {}): StandaloneSession {
    return {
      id: "s-1",
      path: "/tmp/test",
      status: "idle",
      session_id: null,
      last_response: null,
      transcript_summary: null,
      is_open: 1,
      created_at: sqliteNow(),
      updated_at: sqliteNow(),
      ...overrides,
    };
  }

  it("returns 'waiting' for stale standalone executing (>5 min)", () => {
    const staleTime = sqliteDate(new Date(Date.now() - 6 * 60 * 1000));
    expect(getDisplayStatusStandalone(makeSession({ status: "executing", updated_at: staleTime }))).toBe("waiting");
  });

  it("returns executing as-is when recently updated", () => {
    expect(getDisplayStatusStandalone(makeSession({ status: "executing" }))).toBe("executing");
  });

  it("returns executing when updated 4 min ago (not yet stale)", () => {
    const recentTime = sqliteDate(new Date(Date.now() - 4 * 60 * 1000));
    expect(getDisplayStatusStandalone(makeSession({ status: "executing", updated_at: recentTime }))).toBe("executing");
  });

  it("returns idle as-is regardless of staleness", () => {
    const staleTime = sqliteDate(new Date(Date.now() - 10 * 60 * 1000));
    expect(getDisplayStatusStandalone(makeSession({ status: "idle", updated_at: staleTime }))).toBe("idle");
  });
});

describe("normalizeSummary", () => {
  it("strips heading markers", () => {
    expect(normalizeSummary("## Summary\n\nDetails here")).toBe("Summary Details here");
  });

  it("strips bold markers but keeps inner text", () => {
    expect(normalizeSummary("**Validate**: typecheck")).toBe("Validate: typecheck");
  });

  it("strips bullet list markers", () => {
    expect(normalizeSummary("- item one\n- item two")).toBe("item one item two");
  });

  it("strips numbered list markers", () => {
    expect(normalizeSummary("1. first\n2. second")).toBe("first second");
  });

  it("strips blockquote markers", () => {
    expect(normalizeSummary("> quoted text")).toBe("quoted text");
  });

  it("strips inline code backticks but keeps inner text", () => {
    expect(normalizeSummary("run `pnpm test` now")).toBe("run pnpm test now");
  });

  it("collapses multiple newlines into a single space", () => {
    expect(normalizeSummary("line1\n\n\nline2")).toBe("line1 line2");
  });

  it("preserves emoji characters", () => {
    expect(normalizeSummary("typecheck ✅, lint ✅")).toBe("typecheck ✅, lint ✅");
  });

  it("respects the maxChars cap", () => {
    const long = "a".repeat(500);
    expect(normalizeSummary(long, 50)).toHaveLength(50);
  });

  it("handles the full multiline markdown case from the bug report", () => {
    const input = "Draft PR created: **https://github.com/Gridsight/gridsight/pull/13454**\n\n## Summary\n\n- **Validate**: typecheck ✅, my lint ✅ (auth.setup.ts)";
    const result = normalizeSummary(input);
    expect(result).not.toContain("\n");
    expect(result).not.toContain("**");
    expect(result).not.toContain("##");
    expect(result).toContain("Summary");
    expect(result).toContain("Validate");
    expect(result).toContain("✅");
  });
});
