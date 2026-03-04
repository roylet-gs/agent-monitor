import { getWorktreeByPath, upsertAgentStatus } from "../lib/db.js";
import { log } from "../lib/logger.js";
import { publishMessage } from "../lib/pubsub-client.js";
import type { AgentStatusType, HookEvent } from "../lib/types.js";

export async function handleHookEvent(
  worktreePath: string,
  eventOverride?: string
): Promise<void> {
  // Read stdin
  let stdinData = "";
  try {
    stdinData = await readStdin();
  } catch (err) {
    log("debug", "hook-event", `Failed to read stdin: ${err}`);
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
    log("debug", "hook-event", `Worktree not found in DB for path: ${worktreePath}`);
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

  // Fire-and-forget publish for instant TUI update
  await publishMessage({
    type: "agent-status-update",
    worktreeId: worktree.id,
    status,
    sessionId,
    lastResponse,
    transcriptSummary,
    updatedAt: new Date().toISOString(),
  }).catch(() => {});
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
    // stop_hook_active is a loop-prevention flag (true = a Stop hook already
    // blocked once). Since our hook never blocks, this is always false,
    // so Stop always maps to "idle", which is correct: the turn is finished.
    return event.stop_hook_active ? "waiting" : "idle";
  }
  if (event.event === "Notification") {
    // Permission prompts → waiting; other notifications are informational,
    // don't change status
    if (
      event.notification_type === "permission_prompt" ||
      event.notification_type === "elicitation_dialog"
    ) {
      return "waiting";
    }
    return null;
  }
  if (event.event === "SessionStart" || event.event === "SessionEnd") {
    log("debug", "hook-event", `${event.event} → idle`);
    return "idle";
  }

  // Prompt submit → immediately show executing (or planning if in plan mode)
  if (event.event === "UserPromptSubmit") {
    const status = event.permission_mode === "plan" ? "planning" : "executing";
    log("debug", "hook-event", `UserPromptSubmit → ${status} (permission_mode=${event.permission_mode ?? "default"})`);
    return status;
  }

  // Tools that block on user input → waiting
  if (
    event.tool_name === "AskUserQuestion" ||
    event.tool_name === "EnterPlanMode" ||
    event.tool_name === "ExitPlanMode"
  ) {
    log("debug", "hook-event", `Tool ${event.tool_name} → waiting`);
    return "waiting";
  }

  // Plan mode folds into the planning status
  if (event.permission_mode === "plan") {
    log("debug", "hook-event", `Plan mode (event=${event.event}) → planning`);
    return "planning";
  }

  // PreToolUse/PostToolUse/Subagent events → actively working
  if (
    event.event === "PreToolUse" ||
    event.event === "PostToolUse" ||
    event.event === "SubagentStart" ||
    event.event === "SubagentStop"
  ) {
    log("debug", "hook-event", `${event.event} → executing`);
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
