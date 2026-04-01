import { describe, it, expect, vi } from "vitest";

vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

// Mock db to avoid real DB access
vi.mock("../../src/lib/db.js", () => ({
  insertPendingInput: vi.fn(),
  removePendingInput: vi.fn(),
}));

describe("shouldBlockForInput", () => {
  let shouldBlockForInput: typeof import("../../src/lib/managed-bridge.js").shouldBlockForInput;

  beforeEach(async () => {
    ({ shouldBlockForInput } = await import("../../src/lib/managed-bridge.js"));
  });

  it("returns true for PreToolUse with AskUserQuestion", () => {
    expect(shouldBlockForInput({
      event: "PreToolUse",
      tool_name: "AskUserQuestion",
    })).toBe(true);
  });

  it("returns true for PermissionRequest event", () => {
    expect(shouldBlockForInput({
      event: "PermissionRequest",
    })).toBe(true);
  });

  it("returns true for PreToolUse with permission_prompt", () => {
    expect(shouldBlockForInput({
      event: "PreToolUse",
      tool_name: "Bash",
      permission_prompt: true,
    })).toBe(true);
  });

  it("returns false for regular PreToolUse", () => {
    expect(shouldBlockForInput({
      event: "PreToolUse",
      tool_name: "Read",
    })).toBe(false);
  });

  it("returns false for PostToolUse", () => {
    expect(shouldBlockForInput({
      event: "PostToolUse",
      tool_name: "AskUserQuestion",
    })).toBe(false);
  });

  it("returns false for Stop event", () => {
    expect(shouldBlockForInput({
      event: "Stop",
    })).toBe(false);
  });

  it("returns false for Notification event", () => {
    expect(shouldBlockForInput({
      event: "Notification",
      notification_type: "permission_prompt",
    })).toBe(false);
  });
});
