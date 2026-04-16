import { getWorktreeByPath, getAgentStatus, upsertAgentStatus, getStandaloneSessionByPath, upsertStandaloneSession, touchAgentStatusTimestamp, touchStandaloneSessionTimestamp } from "../lib/db.js";
import { getWorktreeRoot } from "../lib/git.js";
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
  const p = payload as unknown as Record<string, unknown>;
  const debugFields: Record<string, unknown> = {};
  if (p.session_id) debugFields.session_id = p.session_id;
  if (p.reason) debugFields.reason = p.reason;
  if (p.notification_type) debugFields.notification_type = p.notification_type;
  if (p.message) debugFields.message = String(p.message).slice(0, 100);
  if (p.tool_input) debugFields.tool_input = JSON.stringify(p.tool_input).slice(0, 200);
  log("debug", "hook-event", `Payload details: ${JSON.stringify(debugFields)}`);

  // Resolve to git worktree root so subdirectory paths match the DB
  const resolvedPath = getWorktreeRoot(worktreePath) ?? worktreePath;
  const worktree = getWorktreeByPath(resolvedPath);
  if (!worktree) {
    // Track as standalone session (non-worktree Claude instance)
    log("debug", "hook-event", `Worktree not found in DB for path: ${worktreePath} (resolved: ${resolvedPath}), tracking as standalone session`);
    await handleStandaloneSession(resolvedPath, payload);
    return;
  }

  // Fetch current status before mapping so Stop can check if agent was active
  const current = getAgentStatus(worktree.id);

  // Map event to status
  const status = mapEventToStatus(payload, current?.status);
  const sessionId = payload.session_id ?? null;
  const lastResponse = extractLastResponse(payload);
  const transcriptSummary = payload.transcript_summary ?? null;
  const isOpen = payload.event === "SessionEnd" ? false : true;

  if (status === null) {
    log("debug", "hook-event", `Skipped status update for ${worktreePath} (informational ${payload.event})`);
    return;
  }

  // Skip redundant DB write + pub/sub if status hasn't changed and there's no new content
  const currentIsOpen = current ? !!current.is_open : false;
  if (current && current.status === status && currentIsOpen === isOpen && !lastResponse && !transcriptSummary) {
    touchAgentStatusTimestamp(worktree.id);
    return;
  }

  upsertAgentStatus(worktree.id, status, sessionId, lastResponse, transcriptSummary, isOpen);
  log("info", "hook-event", `Updated status for ${worktreePath}: ${status} (is_open=${isOpen})`);

  // Fire-and-forget publish for instant TUI update
  await publishMessage({
    type: "agent-status-update",
    worktreeId: worktree.id,
    status,
    sessionId,
    lastResponse,
    transcriptSummary,
    isOpen,
    updatedAt: new Date().toISOString(),
  }).catch(() => {});

  // Detect git push / gh pr create and publish a git-activity message
  // so the TUI can trigger a targeted PR/CI refresh
  if (payload.event === "PostToolUse") {
    const command = String(payload.tool_input?.command ?? "");
    const activity = detectGitActivity(command);
    if (activity) {
      log("info", "hook-event", `Detected git activity: ${activity} in command for ${worktreePath}`);
      await publishMessage({
        type: "git-activity",
        worktreeId: worktree.id,
        repoId: worktree.repo_id,
        branch: worktree.branch,
        activity,
      }).catch(() => {});
    }

  }
}

async function handleStandaloneSession(path: string, payload: HookEvent): Promise<void> {
  const existing = getStandaloneSessionByPath(path);
  const status = mapEventToStatus(payload, existing?.status);
  const sessionId = payload.session_id ?? null;
  const lastResponse = extractLastResponse(payload);
  const transcriptSummary = payload.transcript_summary ?? null;
  const isOpen = payload.event === "SessionEnd" ? false : true;

  if (status === null) {
    log("debug", "hook-event", `Skipped standalone status update for ${path} (informational ${payload.event})`);
    return;
  }

  // Skip redundant write
  const currentIsOpen = existing ? !!existing.is_open : false;
  if (existing && existing.status === status && currentIsOpen === isOpen && !lastResponse && !transcriptSummary) {
    touchStandaloneSessionTimestamp(path);
    return;
  }

  upsertStandaloneSession(path, status, sessionId, lastResponse, transcriptSummary, isOpen);
  log("info", "hook-event", `Updated standalone session for ${path}: ${status} (is_open=${isOpen})`);

  await publishMessage({
    type: "standalone-status-update",
    sessionPath: path,
    status,
    sessionId: sessionId,
    lastResponse,
    transcriptSummary,
    isOpen,
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

export function mapEventToStatus(event: HookEvent, currentStatus?: AgentStatusType | null): AgentStatusType | null {
  // Guard: protect "done" status from late-arriving events.
  // Background subagents fire SubagentStop minutes after the parent session's
  // Stop event, which would incorrectly overwrite "done" back to "executing".
  // Only session-level events (user action or session lifecycle) may transition
  // out of "done".
  if (currentStatus === "done") {
    const allowedAfterDone = new Set(["UserPromptSubmit", "SessionStart", "SessionEnd", "Stop", "StopFailure"]);
    if (!allowedAfterDone.has(event.event)) {
      log("debug", "hook-event", `Ignoring ${event.event} while done (tool=${event.tool_name ?? "none"})`);
      return null;
    }
  }

  // Stop/Notification waiting cases take priority
  if (event.event === "Stop") {
    if (event.stop_hook_active) return "waiting";

    // Preserve waiting state — Stop fires after AskUserQuestion/ExitPlanMode
    // but Claude is still waiting for user input
    if (currentStatus === "waiting") return "waiting";

    // Planning mode Stop = waiting for user feedback on the plan
    if (event.permission_mode === "plan" || currentStatus === "planning") {
      log("debug", "hook-event", `Stop in planning mode → waiting (was ${currentStatus})`);
      return "waiting";
    }

    // Executing → done (Claude finished the task, no stop_hook_active means session ended)
    if (currentStatus === "executing") {
      log("debug", "hook-event", `Stop while executing without stop_hook_active → done`);
      return "done";
    }

    return "idle";
  }
  if (event.event === "StopFailure") {
    log("debug", "hook-event", `StopFailure → idle (turn failed, was ${currentStatus ?? "none"})`);
    return "idle";
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
    if (event.notification_type === "idle_prompt") {
      // idle_prompt after done = task is complete, don't downgrade
      if (currentStatus === "done") {
        log("debug", "hook-event", `idle_prompt after done → preserving done status`);
        return null;
      }
      return "waiting";
    }
    return null;
  }
  // Claude Code 2.x emits a dedicated PermissionRequest event for permission
  // prompts (in addition to / instead of the Notification path above).
  // The hook is fired while Claude blocks on a user decision → waiting.
  if (event.event === "PermissionRequest") {
    log("debug", "hook-event", `PermissionRequest → waiting (was ${currentStatus ?? "none"})`);
    return "waiting";
  }
  if (event.event === "SessionStart" || event.event === "SessionEnd") {
    return "idle";
  }

  // Prompt submit → immediately show executing (or planning if in plan mode)
  if (event.event === "UserPromptSubmit") {
    return event.permission_mode === "plan" ? "planning" : "executing";
  }

  // Tools that block on user input → waiting
  if (
    event.tool_name === "AskUserQuestion" ||
    event.tool_name === "ExitPlanMode"
  ) {
    return "waiting";
  }

  // EnterPlanMode: transitioning to plan mode (not waiting for user input)
  if (event.tool_name === "EnterPlanMode") {
    return "planning";
  }

  // PreToolUse with permission_prompt → waiting (safety net for prompts
  // that don't fire a Notification event)
  if (event.event === "PreToolUse" && event.permission_prompt === true) {
    return "waiting";
  }

  // Plan mode folds into the planning status
  if (event.permission_mode === "plan") {
    return "planning";
  }

  // Tool/subagent events don't always carry permission_mode — preserve
  // "planning" if the agent is already in plan mode, otherwise "executing"
  if (
    (event.event === "PreToolUse" ||
     event.event === "PostToolUse" ||
     event.event === "SubagentStart" ||
     event.event === "SubagentStop") &&
    currentStatus === "planning"
  ) {
    log("debug", "hook-event", `${event.event} during planning → preserving planning status`);
    return "planning";
  }

  // PreToolUse/PostToolUse/Subagent events → actively working
  if (
    event.event === "PreToolUse" ||
    event.event === "PostToolUse" ||
    event.event === "SubagentStart" ||
    event.event === "SubagentStop"
  ) {
    return "executing";
  }

  return "idle";
}

export function detectGitActivity(command: string): "push" | "pr-create" | null {
  if (!command) return null;
  if (/\bgit\s+push\b/.test(command)) return "push";
  if (/\bgh\s+pr\s+create\b/.test(command)) return "pr-create";
  return null;
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
