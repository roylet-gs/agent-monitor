import { describe, it, expect } from "vitest";
import { isEffectivelyOpen, getDisplayStatus, getDisplayStatusStandalone } from "../../src/lib/agent-utils.js";
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

  it("returns executing as-is regardless of staleness", () => {
    const staleTime = sqliteDate(new Date(Date.now() - 200 * 1000));
    expect(getDisplayStatus(makeStatus({ status: "executing", updated_at: staleTime }))).toBe("executing");
  });

  it("returns planning as-is regardless of staleness", () => {
    const staleTime = sqliteDate(new Date(Date.now() - 200 * 1000));
    expect(getDisplayStatus(makeStatus({ status: "planning", updated_at: staleTime }))).toBe("planning");
  });

  it("returns 'idle' as-is regardless of staleness", () => {
    const staleTime = sqliteDate(new Date(Date.now() - 200 * 1000));
    expect(getDisplayStatus(makeStatus({ status: "idle", updated_at: staleTime }))).toBe("idle");
  });

  it("returns 'waiting' as-is regardless of staleness", () => {
    const staleTime = sqliteDate(new Date(Date.now() - 200 * 1000));
    expect(getDisplayStatus(makeStatus({ status: "waiting", updated_at: staleTime }))).toBe("waiting");
  });

  it("returns 'done' as-is regardless of staleness", () => {
    const staleTime = sqliteDate(new Date(Date.now() - 200 * 1000));
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

  it("returns executing as-is regardless of staleness", () => {
    const staleTime = sqliteDate(new Date(Date.now() - 200 * 1000));
    expect(getDisplayStatusStandalone(makeSession({ status: "executing", updated_at: staleTime }))).toBe("executing");
  });

  it("returns actual status when recently updated", () => {
    expect(getDisplayStatusStandalone(makeSession({ status: "executing" }))).toBe("executing");
  });

  it("returns idle as-is regardless of staleness", () => {
    const staleTime = sqliteDate(new Date(Date.now() - 200 * 1000));
    expect(getDisplayStatusStandalone(makeSession({ status: "idle", updated_at: staleTime }))).toBe("idle");
  });
});
