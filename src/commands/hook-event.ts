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

  log("info", "hook-event", `event=${payload.event} tool=${payload.tool_name ?? "none"} stop_hook_active=${payload.stop_hook_active ?? "N/A"} permission_prompt=${payload.permission_prompt ?? "N/A"} permission_mode=${payload.permission_mode ?? "N/A"} for ${worktreePath}`);
  log("debug", "hook-event", `Full payload: ${JSON.stringify(payload).slice(0, 500)}`);

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
  const transcriptSummary = payload.transcript_summary ?? null;

  if (status === null) {
    log("debug", "hook-event", `Skipped status update for ${worktreePath} (informational ${payload.event})`);
    return;
  }

  upsertAgentStatus(worktree.id, status, sessionId, lastResponse, transcriptSummary);
  log("info", "hook-event", `Updated status for ${worktreePath}: ${status}`);
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

function mapEventToStatus(event: HookEvent): AgentStatusType | null {
  // Stop/Notification waiting cases take priority
  if (event.event === "Stop") {
    return event.stop_hook_active ? "waiting" : "idle";
  }
  if (event.event === "Notification") {
    // Permission prompts → waiting; other notifications are informational,
    // don't change status
    return event.notification_type === "permission_prompt" ? "waiting" : null;
  }
  if (event.event === "SessionStart") {
    return "idle";
  }

  // Tools that block on user input → waiting
  if (event.tool_name === "AskUserQuestion" || event.tool_name === "EnterPlanMode") {
    return "waiting";
  }

  // Plan mode folds into the planning status
  if (event.permission_mode === "plan") {
    return "planning";
  }

  // PreToolUse/PostToolUse → actively working
  if (event.event === "PreToolUse" || event.event === "PostToolUse") {
    return "executing";
  }

  return "idle";
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
