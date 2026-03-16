import { describe, it, expect, vi, beforeEach } from "vitest";

// Mock logger to avoid file I/O during tests
vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

// Mock pubsub-client so we can verify published messages
vi.mock("../../src/lib/pubsub-client.js", () => ({
  publishMessage: vi.fn().mockResolvedValue(undefined),
}));

// Mock git module so getWorktreeRoot returns the path as-is
vi.mock("../../src/lib/git.js", () => ({
  getWorktreeRoot: vi.fn((p: string) => p),
}));

// Mock rules module
vi.mock("../../src/lib/rules.js", () => ({
  loadRules: vi.fn().mockReturnValue([]),
  addRule: vi.fn(),
  parseClaudePermissionRule: vi.fn(),
  applyRulesToClaudeSettings: vi.fn(),
}));

// Mock settings module
vi.mock("../../src/lib/settings.js", () => ({
  loadSettings: vi.fn().mockReturnValue({}),
}));

describe("handleStandaloneSession", () => {
  let handleHookEvent: typeof import("../../src/commands/hook-event.js").handleHookEvent;
  let db: typeof import("../../src/lib/db.js");
  let publishMessage: ReturnType<typeof vi.fn>;

  beforeEach(async () => {
    db = await import("../../src/lib/db.js");
    ({ handleHookEvent } = await import("../../src/commands/hook-event.js"));
    ({ publishMessage } = await import("../../src/lib/pubsub-client.js"));
    vi.mocked(publishMessage).mockClear();

    // Simulate stdin by mocking process.stdin.isTTY
    Object.defineProperty(process.stdin, "isTTY", { value: true, configurable: true });
  });

  it("creates a standalone session when worktree not found in DB", async () => {
    // Path not in any repo/worktree - should become standalone
    await handleHookEvent("/tmp/standalone-project", "UserPromptSubmit");
    const session = db.getStandaloneSessionByPath("/tmp/standalone-project");
    expect(session).toBeDefined();
    expect(session!.status).toBe("executing");
    expect(session!.is_open).toBe(1);
  });

  it("standalone session status maps correctly for executing", async () => {
    await handleHookEvent("/tmp/standalone-project", "UserPromptSubmit");
    const session = db.getStandaloneSessionByPath("/tmp/standalone-project");
    expect(session!.status).toBe("executing");
  });

  it("standalone session maps Stop to idle when no prior status", async () => {
    await handleHookEvent("/tmp/standalone-project", "Stop");
    const session = db.getStandaloneSessionByPath("/tmp/standalone-project");
    expect(session!.status).toBe("idle");
  });

  it("standalone session maps Stop to waiting when previously executing", async () => {
    // First create an executing session
    await handleHookEvent("/tmp/standalone-project", "UserPromptSubmit");
    // Now stop - should transition to waiting since Claude is waiting for user input
    await handleHookEvent("/tmp/standalone-project", "Stop");
    const session = db.getStandaloneSessionByPath("/tmp/standalone-project");
    expect(session!.status).toBe("waiting");
  });

  it("SessionEnd marks standalone session as closed (is_open=0)", async () => {
    await handleHookEvent("/tmp/standalone-project", "UserPromptSubmit");
    await handleHookEvent("/tmp/standalone-project", "SessionEnd");
    const session = db.getStandaloneSessionByPath("/tmp/standalone-project");
    expect(session!.is_open).toBe(0);
  });

  it("publishes standalone-status-update message", async () => {
    await handleHookEvent("/tmp/standalone-project", "UserPromptSubmit");
    expect(publishMessage).toHaveBeenCalledWith(
      expect.objectContaining({
        type: "standalone-status-update",
        sessionPath: "/tmp/standalone-project",
        status: "executing",
        isOpen: true,
      })
    );
  });

  it("skips informational Notification events for standalone sessions", async () => {
    // A Notification without permission_prompt type should be skipped
    await handleHookEvent("/tmp/standalone-project", "Notification");
    const session = db.getStandaloneSessionByPath("/tmp/standalone-project");
    expect(session).toBeUndefined();
  });
});

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

  // Stop → waiting when agent was actively working (Claude is now waiting for user)
  it("Stop while executing -> waiting", () => {
    expect(mapEventToStatus({ event: "Stop" }, "executing")).toBe("waiting");
  });

  it("Stop while planning -> waiting (plan review)", () => {
    expect(mapEventToStatus({ event: "Stop" }, "planning")).toBe("waiting");
  });

  it("Stop while waiting -> waiting (preserves waiting)", () => {
    expect(mapEventToStatus({ event: "Stop" }, "waiting")).toBe("waiting");
  });

  it("Stop with permission_mode=plan while executing -> waiting", () => {
    expect(mapEventToStatus({ event: "Stop", permission_mode: "plan" } as any, "executing")).toBe("waiting");
  });

  it("Stop with permission_mode=plan and no currentStatus -> waiting", () => {
    expect(mapEventToStatus({ event: "Stop", permission_mode: "plan" } as any)).toBe("waiting");
  });

  it("Stop while waiting preserves waiting regardless of stop_hook_active", () => {
    expect(mapEventToStatus({ event: "Stop", stop_hook_active: false }, "waiting")).toBe("waiting");
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

  // Subagent events preserve planning status
  it("SubagentStart while planning -> planning", () => {
    expect(mapEventToStatus({ event: "SubagentStart" }, "planning")).toBe("planning");
  });

  it("SubagentStop while planning -> planning", () => {
    expect(mapEventToStatus({ event: "SubagentStop" }, "planning")).toBe("planning");
  });

  it("SubagentStart while executing -> executing", () => {
    expect(mapEventToStatus({ event: "SubagentStart" }, "executing")).toBe("executing");
  });

  // PreToolUse/PostToolUse preserve planning status
  it("PreToolUse while planning without permission_mode -> planning", () => {
    expect(mapEventToStatus({ event: "PreToolUse", tool_name: "Read" }, "planning")).toBe("planning");
  });

  it("PostToolUse while planning without permission_mode -> planning", () => {
    expect(mapEventToStatus({ event: "PostToolUse", tool_name: "Write" }, "planning")).toBe("planning");
  });

  it("PreToolUse while executing -> executing", () => {
    expect(mapEventToStatus({ event: "PreToolUse", tool_name: "Read" }, "executing")).toBe("executing");
  });

  it("PostToolUse while executing -> executing", () => {
    expect(mapEventToStatus({ event: "PostToolUse", tool_name: "Write" }, "executing")).toBe("executing");
  });

  // Full sequence: EnterPlanMode → tool events → Stop → waiting
  it("EnterPlanMode → PreToolUse → PostToolUse → Stop transitions correctly", () => {
    // EnterPlanMode sets planning
    const s1 = mapEventToStatus({ event: "PostToolUse", tool_name: "EnterPlanMode" });
    expect(s1).toBe("planning");
    // PreToolUse without permission_mode preserves planning
    const s2 = mapEventToStatus({ event: "PreToolUse", tool_name: "Read" }, s1);
    expect(s2).toBe("planning");
    // PostToolUse without permission_mode preserves planning
    const s3 = mapEventToStatus({ event: "PostToolUse", tool_name: "Read" }, s2);
    expect(s3).toBe("planning");
    // Stop from planning → waiting
    const s4 = mapEventToStatus({ event: "Stop" }, s3);
    expect(s4).toBe("waiting");
  });

  // PreToolUse with permission_prompt
  it("PreToolUse with permission_prompt -> waiting", () => {
    expect(
      mapEventToStatus({ event: "PreToolUse", tool_name: "Read", permission_prompt: true })
    ).toBe("waiting");
  });

  it("PreToolUse with permission_prompt in plan mode -> waiting", () => {
    expect(
      mapEventToStatus({ event: "PreToolUse", tool_name: "Read", permission_prompt: true, permission_mode: "plan" })
    ).toBe("waiting");
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
