import { getWorktreeByPath, upsertAgentStatus } from "../lib/db.js";
import { log } from "../lib/logger.js";
import type { AgentStatusType, HookEvent } from "../lib/types.js";

export async function handleHookEvent(
  worktreePath: string,
  eventOverride?: string
): Promise<void> {
  // Read stdin
  let stdinData = "";
  try {
    stdinData = await readStdin();
  } catch {
    // stdin might be empty for some events
  }

  let payload: HookEvent = { event: eventOverride ?? "unknown" };
  if (stdinData.trim()) {
    try {
      const parsed = JSON.parse(stdinData);
      payload = { ...parsed, event: eventOverride ?? parsed.event ?? parsed.hook_event_name ?? "unknown" };
    } catch {
      log("warn", "hook-event", `Failed to parse stdin JSON: ${stdinData.slice(0, 200)}`);
    }
  }

  log("debug", "hook-event", `Received event: ${payload.event} for ${worktreePath}`);

  // Find worktree in DB
  const worktree = getWorktreeByPath(worktreePath);
  if (!worktree) {
    log("warn", "hook-event", `Worktree not found in DB for path: ${worktreePath}`);
    return;
  }

  // Map event to status
  const status = mapEventToStatus(payload);
  const sessionId = payload.session_id ?? null;
  const lastResponse = extractLastResponse(payload);
  const planMode = detectPlanMode(payload);

  upsertAgentStatus(worktree.id, status, sessionId, lastResponse, planMode);
  log("info", "hook-event", `Updated status for ${worktreePath}: ${status}${planMode != null ? ` (plan_mode=${planMode})` : ""}`);
}

function extractLastResponse(event: HookEvent): string | null {
  // Stop event has last_assistant_message — the main response text
  if (event.last_assistant_message) {
    return event.last_assistant_message;
  }
  // Notification has a message field
  if (event.event === "Notification" && event.message) {
    return event.message;
  }
  return null;
}

function detectPlanMode(event: HookEvent): boolean | null {
  const tool = event.tool_name ?? "";

  // PreToolUse for EnterPlanMode → plan mode ON
  if (event.event === "PreToolUse" && tool === "EnterPlanMode") {
    return true;
  }
  // PostToolUse for ExitPlanMode → plan mode OFF
  if (event.event === "PostToolUse" && tool === "ExitPlanMode") {
    return false;
  }
  // SessionStart resets plan mode
  if (event.event === "SessionStart") {
    return false;
  }
  // null = no change
  return null;
}

function mapEventToStatus(event: HookEvent): AgentStatusType {
  switch (event.event) {
    case "SessionStart":
      return "idle";

    case "PreToolUse": {
      const tool = event.tool_name ?? "";
      if (tool === "Agent" || tool === "EnterPlanMode" || tool === "Plan") {
        return "thinking";
      }
      return "executing";
    }

    case "PostToolUse":
      return "executing";

    case "Stop":
      if (event.stop_hook_active) {
        return "waiting_for_input";
      }
      return "idle";

    case "Notification":
      if (event.permission_prompt) {
        return "waiting_for_input";
      }
      return "executing";

    default:
      return "idle";
  }
}

function readStdin(): Promise<string> {
  return new Promise((resolve, reject) => {
    let data = "";
    const timeout = setTimeout(() => resolve(data), 2000);

    if (process.stdin.isTTY) {
      clearTimeout(timeout);
      resolve("");
      return;
    }

    process.stdin.setEncoding("utf-8");
    process.stdin.on("data", (chunk) => {
      data += chunk;
    });
    process.stdin.on("end", () => {
      clearTimeout(timeout);
      resolve(data);
    });
    process.stdin.on("error", (err) => {
      clearTimeout(timeout);
      reject(err);
    });
  });
}
