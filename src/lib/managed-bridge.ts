import net from "net";
import { randomUUID } from "crypto";
import { existsSync } from "fs";
import { SOCKET_PATH } from "./paths.js";
import { log } from "./logger.js";
import { insertPendingInput, removePendingInput } from "./db.js";
import type { HookEvent, PendingInput } from "./types.js";
import type { PendingInputMessage } from "./pubsub-types.js";

/**
 * Blocking bridge for managed mode.
 *
 * When Claude fires a hook that requires user input (AskUserQuestion or
 * PermissionRequest), this function:
 * 1. Creates a PendingInput record in the DB
 * 2. Connects to the daemon socket
 * 3. Sends a "pending-input" message so the daemon notifies the TUI
 * 4. Blocks until the daemon sends back a "respond-input" message
 * 5. Returns the formatted hook response JSON for Claude Code
 *
 * Falls back gracefully if the daemon socket is unavailable.
 */
export async function waitForManagedResponse(
  worktreeId: string,
  sessionId: string | null,
  payload: HookEvent,
  timeoutMs: number
): Promise<object | null> {
  if (!existsSync(SOCKET_PATH)) {
    log("debug", "managed-bridge", "Daemon socket not found, skipping managed response");
    return null;
  }

  const pendingInput = buildPendingInput(worktreeId, sessionId, payload);
  insertPendingInput(pendingInput);
  log("info", "managed-bridge", `Created pending input ${pendingInput.id} type=${pendingInput.type} for worktree=${worktreeId}`);

  try {
    const response = await connectAndWait(pendingInput, timeoutMs);
    if (response) {
      log("info", "managed-bridge", `Received response for ${pendingInput.id}: type=${pendingInput.type}`);
      return buildHookResponse(pendingInput, response, payload);
    }
    log("warn", "managed-bridge", `Timed out waiting for response to ${pendingInput.id}`);
    return null;
  } finally {
    removePendingInput(pendingInput.id);
  }
}

/**
 * Determines if a hook event should block for managed mode input.
 */
export function shouldBlockForInput(payload: HookEvent): boolean {
  // AskUserQuestion in PreToolUse
  if (payload.event === "PreToolUse" && payload.tool_name === "AskUserQuestion") return true;
  // PermissionRequest event
  if (payload.event === "PermissionRequest") return true;
  // PreToolUse with permission_prompt (safety net)
  if (payload.event === "PreToolUse" && payload.permission_prompt === true) return true;
  return false;
}

function buildPendingInput(worktreeId: string, sessionId: string | null, payload: HookEvent): PendingInput {
  const isQuestion = payload.tool_name === "AskUserQuestion";

  if (isQuestion) {
    const toolInput = payload.tool_input ?? {};
    const questions = toolInput.questions as Array<{ question: string; options?: Array<{ label: string; description?: string }> }> | undefined;
    const firstQuestion = questions?.[0];

    return {
      id: randomUUID(),
      worktreeId,
      sessionId,
      type: "question",
      question: firstQuestion?.question ?? payload.message ?? "Claude is asking a question",
      options: firstQuestion?.options,
      toolInput: payload.tool_input,
      createdAt: new Date().toISOString(),
    };
  }

  // Permission request
  return {
    id: randomUUID(),
    worktreeId,
    sessionId,
    type: "permission",
    question: payload.message ?? `Permission needed for ${payload.tool_name ?? "unknown tool"}`,
    toolName: payload.tool_name,
    toolInput: payload.tool_input,
    createdAt: new Date().toISOString(),
  };
}

interface ManagedResponse {
  response: string;
  decision?: "allow" | "deny";
}

function connectAndWait(
  pendingInput: PendingInput,
  timeoutMs: number
): Promise<ManagedResponse | null> {
  return new Promise((resolve) => {
    const hookConnectionId = randomUUID();
    let settled = false;

    const timer = setTimeout(() => {
      if (!settled) {
        settled = true;
        conn.destroy();
        resolve(null);
      }
    }, timeoutMs);

    const conn = net.createConnection({ path: SOCKET_PATH }, () => {
      // Send pending-input message to daemon
      const msg: PendingInputMessage = {
        type: "pending-input",
        input: pendingInput,
        hookConnectionId,
      };
      conn.write(JSON.stringify(msg) + "\n");
    });

    let buffer = "";
    conn.on("data", (data) => {
      buffer += data.toString();
      const lines = buffer.split("\n");
      buffer = lines.pop() ?? "";

      for (const line of lines) {
        if (!line.trim()) continue;
        try {
          const parsed = JSON.parse(line);
          if (parsed.type === "respond-input" && parsed.inputId === pendingInput.id) {
            if (!settled) {
              settled = true;
              clearTimeout(timer);
              conn.destroy();
              resolve({
                response: parsed.response ?? "",
                decision: parsed.decision,
              });
            }
          }
        } catch {
          log("debug", "managed-bridge", `Failed to parse daemon message: ${line.slice(0, 100)}`);
        }
      }
    });

    conn.on("error", (err) => {
      log("warn", "managed-bridge", `Socket error: ${err.message}`);
      if (!settled) {
        settled = true;
        clearTimeout(timer);
        resolve(null);
      }
    });

    conn.on("close", () => {
      if (!settled) {
        settled = true;
        clearTimeout(timer);
        resolve(null);
      }
    });
  });
}

/**
 * Build the JSON that the hook should write to stdout for Claude Code to consume.
 */
function buildHookResponse(
  pendingInput: PendingInput,
  response: ManagedResponse,
  originalPayload: HookEvent
): object {
  if (pendingInput.type === "question") {
    // For AskUserQuestion, return updatedInput with the answer
    const toolInput = originalPayload.tool_input ?? {};
    const questions = toolInput.questions as Array<Record<string, unknown>> | undefined;
    const firstQuestion = questions?.[0];
    const questionText = (firstQuestion?.question as string) ?? "";

    return {
      hookSpecificOutput: {
        hookEventName: "PreToolUse",
        permissionDecision: "allow",
        updatedInput: {
          ...toolInput,
          answers: { [questionText]: response.response },
        },
      },
    };
  }

  // Permission decision
  if (originalPayload.event === "PermissionRequest") {
    return {
      hookSpecificOutput: {
        hookEventName: "PermissionRequest",
        decision: {
          behavior: response.decision ?? "allow",
          ...(response.decision === "deny" ? { message: response.response || "Denied by agent-monitor" } : {}),
        },
      },
    };
  }

  // PreToolUse permission prompt fallback
  return {
    hookSpecificOutput: {
      hookEventName: "PreToolUse",
      permissionDecision: response.decision ?? "allow",
      permissionDecisionReason: response.response || undefined,
    },
  };
}
