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

  it("standalone session maps Stop to done when previously executing", async () => {
    // First create an executing session
    await handleHookEvent("/tmp/standalone-project", "UserPromptSubmit");
    // Now stop without stop_hook_active - Claude finished the task
    await handleHookEvent("/tmp/standalone-project", "Stop");
    const session = db.getStandaloneSessionByPath("/tmp/standalone-project");
    expect(session!.status).toBe("done");
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

  it("tracks active_subagents and reports delegating while background agents run", async () => {
    await handleHookEvent("/tmp/standalone-project", "UserPromptSubmit"); // executing, count 0
    await handleHookEvent("/tmp/standalone-project", "SubagentStart"); // count 1
    await handleHookEvent("/tmp/standalone-project", "SubagentStart"); // count 2

    let session = db.getStandaloneSessionByPath("/tmp/standalone-project");
    expect(session!.active_subagents).toBe(2);

    // Main turn stops while 2 subagents still running → delegating, not idle/done
    await handleHookEvent("/tmp/standalone-project", "Stop");
    session = db.getStandaloneSessionByPath("/tmp/standalone-project");
    expect(session!.status).toBe("delegating");
    expect(session!.active_subagents).toBe(2);

    // First subagent finishes → still delegating
    await handleHookEvent("/tmp/standalone-project", "SubagentStop");
    session = db.getStandaloneSessionByPath("/tmp/standalone-project");
    expect(session!.active_subagents).toBe(1);
    expect(session!.status).toBe("delegating");

    // Last subagent finishes → all work done
    await handleHookEvent("/tmp/standalone-project", "SubagentStop");
    session = db.getStandaloneSessionByPath("/tmp/standalone-project");
    expect(session!.active_subagents).toBe(0);
    expect(session!.status).toBe("done");
  });
});

describe("session id updates", () => {
  let handleHookEvent: typeof import("../../src/commands/hook-event.js").handleHookEvent;
  let db: typeof import("../../src/lib/db.js");

  // Feed a payload through the real stdin-reading path: readStdin attaches its
  // listeners synchronously, so emitting data/end right after the call works.
  function sendEvent(path: string, payload: Record<string, unknown>): Promise<void> {
    Object.defineProperty(process.stdin, "isTTY", { value: false, configurable: true });
    const done = handleHookEvent(path);
    process.stdin.emit("data", JSON.stringify(payload));
    process.stdin.emit("end");
    return done;
  }

  beforeEach(async () => {
    db = await import("../../src/lib/db.js");
    ({ handleHookEvent } = await import("../../src/commands/hook-event.js"));
  });

  it("worktree: a new session id is written even when status is unchanged", async () => {
    const repo = db.addRepository("/tmp/sess-repo", "sess-repo");
    const wt = db.upsertWorktree(repo.id, "/tmp/sess-repo", "main", "main", true);

    await sendEvent("/tmp/sess-repo", { hook_event_name: "UserPromptSubmit", session_id: "sess-a" });
    expect(db.getAgentStatus(wt.id)!.session_id).toBe("sess-a");

    // Same status (executing), no response/summary — previously skipped as redundant
    await sendEvent("/tmp/sess-repo", { hook_event_name: "UserPromptSubmit", session_id: "sess-b" });
    expect(db.getAgentStatus(wt.id)!.session_id).toBe("sess-b");
  });

  it("standalone: a new session id is written even when status is unchanged", async () => {
    const path = "/tmp/standalone-sess-change";
    await sendEvent(path, { hook_event_name: "UserPromptSubmit", session_id: "sess-a" });
    expect(db.getStandaloneSessionByPath(path)!.session_id).toBe("sess-a");

    await sendEvent(path, { hook_event_name: "UserPromptSubmit", session_id: "sess-b" });
    expect(db.getStandaloneSessionByPath(path)!.session_id).toBe("sess-b");
  });

  it("event without a session id keeps the existing one", async () => {
    const path = "/tmp/standalone-sess-keep";
    await sendEvent(path, { hook_event_name: "UserPromptSubmit", session_id: "sess-a" });
    await sendEvent(path, { hook_event_name: "PreToolUse", tool_name: "Read" });
    expect(db.getStandaloneSessionByPath(path)!.session_id).toBe("sess-a");
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
  it("Stop while executing -> done", () => {
    expect(mapEventToStatus({ event: "Stop" }, "executing")).toBe("done");
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

  // acceptEdits mode (after user approves plan) — acceptEdits is the normal
  // working mode, not a planning mode, so Stop follows standard rules
  it("Stop with permission_mode=acceptEdits -> idle", () => {
    expect(mapEventToStatus({ event: "Stop", permission_mode: "acceptEdits" } as any)).toBe("idle");
  });

  it("Stop with permission_mode=acceptEdits while executing -> done", () => {
    expect(mapEventToStatus({ event: "Stop", permission_mode: "acceptEdits" } as any, "executing")).toBe("done");
  });

  it("UserPromptSubmit with permission_mode=acceptEdits -> executing", () => {
    expect(
      mapEventToStatus({ event: "UserPromptSubmit", permission_mode: "acceptEdits" } as any)
    ).toBe("executing");
  });

  it("PreToolUse with permission_mode=acceptEdits -> executing", () => {
    expect(
      mapEventToStatus({ event: "PreToolUse", tool_name: "Edit", permission_mode: "acceptEdits" } as any)
    ).toBe("executing");
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

  it("Notification with idle_prompt -> waiting", () => {
    expect(
      mapEventToStatus({ event: "Notification", notification_type: "idle_prompt" })
    ).toBe("waiting");
  });

  it("Notification with idle_prompt after done -> null (preserve done)", () => {
    expect(
      mapEventToStatus({ event: "Notification", notification_type: "idle_prompt" }, "done")
    ).toBe(null);
  });

  it("Notification with other type -> null (skip)", () => {
    expect(
      mapEventToStatus({ event: "Notification", notification_type: "info" })
    ).toBe(null);
  });

  // PermissionRequest event (Claude Code 2.x managed hook)
  it("PermissionRequest -> waiting", () => {
    expect(mapEventToStatus({ event: "PermissionRequest" })).toBe("waiting");
  });

  it("PermissionRequest while executing -> waiting", () => {
    expect(mapEventToStatus({ event: "PermissionRequest" }, "executing")).toBe("waiting");
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

  it("PreToolUse EnterPlanMode -> planning", () => {
    expect(mapEventToStatus({ event: "PreToolUse", tool_name: "EnterPlanMode" })).toBe(
      "planning"
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

  // StopFailure event
  it("StopFailure -> idle", () => {
    expect(mapEventToStatus({ event: "StopFailure" })).toBe("idle");
  });

  it("StopFailure while executing -> idle", () => {
    expect(mapEventToStatus({ event: "StopFailure" }, "executing")).toBe("idle");
  });

  it("StopFailure while planning -> idle", () => {
    expect(mapEventToStatus({ event: "StopFailure" }, "planning")).toBe("idle");
  });

  it("StopFailure while done -> idle (allowed through guard)", () => {
    expect(mapEventToStatus({ event: "StopFailure" }, "done")).toBe("idle");
  });

  // "done" status protection — late-arriving events must not overwrite "done"
  it("SubagentStop while done -> null (protected)", () => {
    expect(mapEventToStatus({ event: "SubagentStop" }, "done")).toBe(null);
  });

  it("SubagentStart while done -> null (protected)", () => {
    expect(mapEventToStatus({ event: "SubagentStart" }, "done")).toBe(null);
  });

  it("PreToolUse while done -> null (protected)", () => {
    expect(mapEventToStatus({ event: "PreToolUse", tool_name: "Bash" }, "done")).toBe(null);
  });

  it("PostToolUse while done -> null (protected)", () => {
    expect(mapEventToStatus({ event: "PostToolUse", tool_name: "Bash" }, "done")).toBe(null);
  });

  it("Notification while done -> null (protected)", () => {
    expect(mapEventToStatus({ event: "Notification", notification_type: "info" }, "done")).toBe(null);
  });

  it("PermissionRequest while done -> null (protected)", () => {
    expect(mapEventToStatus({ event: "PermissionRequest" }, "done")).toBe(null);
  });

  // Events that CAN transition out of "done"
  it("UserPromptSubmit while done -> executing (allowed)", () => {
    expect(mapEventToStatus({ event: "UserPromptSubmit" }, "done")).toBe("executing");
  });

  it("SessionStart while done -> idle (allowed)", () => {
    expect(mapEventToStatus({ event: "SessionStart" }, "done")).toBe("idle");
  });

  it("SessionEnd while done -> idle (allowed)", () => {
    expect(mapEventToStatus({ event: "SessionEnd" }, "done")).toBe("idle");
  });

  // Unknown events
  it("unknown event -> idle", () => {
    expect(mapEventToStatus({ event: "SomethingElse" })).toBe("idle");
  });

  // Background subagents outstanding — main turn stopped but work continues
  it("Stop with 1 subagent outstanding -> delegating (instead of idle)", () => {
    expect(mapEventToStatus({ event: "Stop" }, "executing", 1)).toBe("delegating");
  });

  it("Stop in plan mode with subagents outstanding -> delegating (instead of waiting)", () => {
    expect(
      mapEventToStatus({ event: "Stop", permission_mode: "plan" } as any, "planning", 2)
    ).toBe("delegating");
  });

  it("Stop with stop_hook_active still wins over subagents -> waiting", () => {
    expect(mapEventToStatus({ event: "Stop", stop_hook_active: true }, "executing", 1)).toBe("waiting");
  });

  it("Notification idle_prompt with subagents outstanding -> delegating", () => {
    expect(
      mapEventToStatus({ event: "Notification", notification_type: "idle_prompt" }, "delegating", 2)
    ).toBe("delegating");
  });

  it("permission prompt with subagents outstanding still -> waiting (needs user)", () => {
    expect(
      mapEventToStatus({ event: "Notification", notification_type: "permission_prompt" }, "delegating", 1)
    ).toBe("waiting");
  });

  it("PermissionRequest with subagents outstanding still -> waiting", () => {
    expect(mapEventToStatus({ event: "PermissionRequest" }, "delegating", 1)).toBe("waiting");
  });

  // delegating transition-out
  it("SubagentStop while delegating with count still >0 -> delegating", () => {
    expect(mapEventToStatus({ event: "SubagentStop" }, "delegating", 1)).toBe("delegating");
  });

  it("SubagentStop while delegating with count 0 -> done (all subagents finished)", () => {
    expect(mapEventToStatus({ event: "SubagentStop" }, "delegating", 0)).toBe("done");
  });

  it("SubagentStart while delegating -> delegating", () => {
    expect(mapEventToStatus({ event: "SubagentStart" }, "delegating", 2)).toBe("delegating");
  });

  it("Stop while delegating with subagents still running -> delegating", () => {
    expect(mapEventToStatus({ event: "Stop" }, "delegating", 1)).toBe("delegating");
  });

  it("Stop while delegating with count 0 -> done", () => {
    expect(mapEventToStatus({ event: "Stop" }, "delegating", 0)).toBe("done");
  });

  it("UserPromptSubmit while delegating -> executing (main agent active again)", () => {
    expect(mapEventToStatus({ event: "UserPromptSubmit" }, "delegating", 1)).toBe("executing");
  });

  it("PreToolUse while delegating -> executing (main agent working)", () => {
    expect(mapEventToStatus({ event: "PreToolUse", tool_name: "Read" }, "delegating", 1)).toBe("executing");
  });

  it("Stop with 0 subagents behaves as before (executing -> done)", () => {
    expect(mapEventToStatus({ event: "Stop" }, "executing", 0)).toBe("done");
  });
});

describe("computeSubagentCount", () => {
  let computeSubagentCount: typeof import("../../src/commands/hook-event.js").computeSubagentCount;

  beforeEach(async () => {
    ({ computeSubagentCount } = await import("../../src/commands/hook-event.js"));
  });

  it("SubagentStart increments", () => {
    expect(computeSubagentCount({ event: "SubagentStart" }, 0)).toBe(1);
    expect(computeSubagentCount({ event: "SubagentStart" }, 2)).toBe(3);
  });

  it("SubagentStop decrements", () => {
    expect(computeSubagentCount({ event: "SubagentStop" }, 2)).toBe(1);
  });

  it("SubagentStop floors at 0", () => {
    expect(computeSubagentCount({ event: "SubagentStop" }, 0)).toBe(0);
  });

  it("SessionStart / SessionEnd reset to 0", () => {
    expect(computeSubagentCount({ event: "SessionStart" }, 5)).toBe(0);
    expect(computeSubagentCount({ event: "SessionEnd" }, 5)).toBe(0);
  });

  it("other events leave the count unchanged", () => {
    expect(computeSubagentCount({ event: "Stop" }, 3)).toBe(3);
    expect(computeSubagentCount({ event: "PreToolUse", tool_name: "Read" }, 3)).toBe(3);
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
