import { describe, it, expect, vi, beforeEach } from "vitest";

// Mock logger to avoid file I/O during tests
vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

describe("mapEventToStatus", () => {
  let mapEventToStatus: typeof import("../../src/commands/hook-event.js").mapEventToStatus;

  beforeEach(async () => {
    ({ mapEventToStatus } = await import("../../src/commands/hook-event.js"));
  });

  // Stop events
  it("Stop -> idle", () => {
    expect(mapEventToStatus({ event: "Stop" })).toBe("idle");
  });

  it("Stop with stop_hook_active -> waiting", () => {
    expect(mapEventToStatus({ event: "Stop", stop_hook_active: true })).toBe("waiting");
  });

  it("Stop with stop_hook_active=false -> idle", () => {
    expect(mapEventToStatus({ event: "Stop", stop_hook_active: false })).toBe("idle");
  });

  // Stop → done when agent was actively working
  it("Stop while executing -> done", () => {
    expect(mapEventToStatus({ event: "Stop" }, "executing")).toBe("done");
  });

  it("Stop while planning -> done", () => {
    expect(mapEventToStatus({ event: "Stop" }, "planning")).toBe("done");
  });

  it("Stop while waiting -> idle (not done)", () => {
    expect(mapEventToStatus({ event: "Stop" }, "waiting")).toBe("idle");
  });

  it("Stop while idle -> idle (not done)", () => {
    expect(mapEventToStatus({ event: "Stop" }, "idle")).toBe("idle");
  });

  it("Stop while done -> idle (not done)", () => {
    expect(mapEventToStatus({ event: "Stop" }, "done")).toBe("idle");
  });

  it("Stop with stop_hook_active still returns waiting regardless of currentStatus", () => {
    expect(mapEventToStatus({ event: "Stop", stop_hook_active: true }, "executing")).toBe("waiting");
  });

  // Notification events
  it("Notification with permission_prompt -> waiting", () => {
    expect(
      mapEventToStatus({ event: "Notification", notification_type: "permission_prompt" })
    ).toBe("waiting");
  });

  it("Notification with elicitation_dialog -> waiting", () => {
    expect(
      mapEventToStatus({ event: "Notification", notification_type: "elicitation_dialog" })
    ).toBe("waiting");
  });

  it("Notification without permission_prompt -> null (skip)", () => {
    expect(mapEventToStatus({ event: "Notification" })).toBe(null);
  });

  it("Notification with other type -> null (skip)", () => {
    expect(
      mapEventToStatus({ event: "Notification", notification_type: "info" })
    ).toBe(null);
  });

  // Session events
  it("SessionStart -> idle", () => {
    expect(mapEventToStatus({ event: "SessionStart" })).toBe("idle");
  });

  it("SessionEnd -> idle", () => {
    expect(mapEventToStatus({ event: "SessionEnd" })).toBe("idle");
  });

  // UserPromptSubmit
  it("UserPromptSubmit -> executing", () => {
    expect(mapEventToStatus({ event: "UserPromptSubmit" })).toBe("executing");
  });

  it("UserPromptSubmit with plan mode -> planning", () => {
    expect(
      mapEventToStatus({ event: "UserPromptSubmit", permission_mode: "plan" })
    ).toBe("planning");
  });

  it("UserPromptSubmit with default mode -> executing", () => {
    expect(
      mapEventToStatus({ event: "UserPromptSubmit", permission_mode: "default" })
    ).toBe("executing");
  });

  // Waiting tools
  it("AskUserQuestion tool -> waiting", () => {
    expect(mapEventToStatus({ event: "PreToolUse", tool_name: "AskUserQuestion" })).toBe(
      "waiting"
    );
  });

  it("PreToolUse EnterPlanMode -> waiting", () => {
    expect(mapEventToStatus({ event: "PreToolUse", tool_name: "EnterPlanMode" })).toBe(
      "waiting"
    );
  });

  it("PostToolUse EnterPlanMode -> planning", () => {
    expect(mapEventToStatus({ event: "PostToolUse", tool_name: "EnterPlanMode" })).toBe(
      "planning"
    );
  });

  it("ExitPlanMode tool -> waiting", () => {
    expect(mapEventToStatus({ event: "PreToolUse", tool_name: "ExitPlanMode" })).toBe(
      "waiting"
    );
  });

  // Plan mode with any event
  it("PreToolUse with plan mode -> planning", () => {
    expect(
      mapEventToStatus({ event: "PreToolUse", tool_name: "Read", permission_mode: "plan" })
    ).toBe("planning");
  });

  it("PostToolUse with plan mode -> planning", () => {
    expect(
      mapEventToStatus({ event: "PostToolUse", tool_name: "Read", permission_mode: "plan" })
    ).toBe("planning");
  });

  // Executing events
  it("PreToolUse -> executing", () => {
    expect(mapEventToStatus({ event: "PreToolUse", tool_name: "Read" })).toBe("executing");
  });

  it("PostToolUse -> executing", () => {
    expect(mapEventToStatus({ event: "PostToolUse", tool_name: "Write" })).toBe("executing");
  });

  it("SubagentStart -> executing", () => {
    expect(mapEventToStatus({ event: "SubagentStart" })).toBe("executing");
  });

  it("SubagentStop -> executing", () => {
    expect(mapEventToStatus({ event: "SubagentStop" })).toBe("executing");
  });

  // Unknown events
  it("unknown event -> idle", () => {
    expect(mapEventToStatus({ event: "SomethingElse" })).toBe("idle");
  });
});

describe("detectGitActivity", () => {
  let detectGitActivity: typeof import("../../src/commands/hook-event.js").detectGitActivity;

  beforeEach(async () => {
    ({ detectGitActivity } = await import("../../src/commands/hook-event.js"));
  });

  it("detects git push", () => {
    expect(detectGitActivity("git push origin main")).toBe("push");
  });

  it("detects git push with flags", () => {
    expect(detectGitActivity("git push --force origin main")).toBe("push");
  });

  it("detects gh pr create", () => {
    expect(detectGitActivity('gh pr create --title "my pr"')).toBe("pr-create");
  });

  it("returns null for other commands", () => {
    expect(detectGitActivity("git status")).toBe(null);
  });

  it("returns null for empty string", () => {
    expect(detectGitActivity("")).toBe(null);
  });

  it("returns null for git pull", () => {
    expect(detectGitActivity("git pull")).toBe(null);
  });
});
