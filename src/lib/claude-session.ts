/**
 * Managed Claude Code sessions: am starts one headless claude session per
 * worktree and drives it turn-by-turn.
 *
 * Each turn spawns a detached `claude -p` process (so it survives the TUI
 * exiting) that appends stream-json events to a per-session JSONL log file.
 * That log file is the transcript store; SQLite (managed_sessions) holds the
 * session UUID, the in-flight turn pid, and the turn count. The session UUID
 * is claude's own session id, so `claude --resume <id>` attaches to the same
 * conversation interactively.
 */

import { spawn } from "child_process";
import { appendFileSync, existsSync, mkdirSync, openSync, closeSync, readFileSync } from "fs";
import { join } from "path";
import { randomUUID } from "crypto";
import { SESSIONS_DIR } from "./paths.js";
import {
  createManagedSession,
  getManagedSession,
  recordManagedSessionTurn,
  clearManagedSessionTurnPid,
} from "./db.js";
import { publishMessage } from "./pubsub-client.js";
import { log } from "./logger.js";
import type { ChatMessage, ManagedSession, Settings, Worktree } from "./types.js";

export function sessionLogPath(sessionId: string): string {
  return join(SESSIONS_DIR, `${sessionId}.jsonl`);
}

export function isTurnRunning(session: ManagedSession): boolean {
  if (!session.turn_pid) return false;
  try {
    process.kill(session.turn_pid, 0);
    return true;
  } catch {
    return false;
  }
}

/**
 * Build the argv for one headless turn. The first turn creates the session
 * with a pre-assigned UUID; later turns resume it.
 */
export function buildTurnArgs(session: ManagedSession, prompt: string, settings: Settings): string[] {
  const args = ["-p", "--output-format", "stream-json", "--verbose"];
  if (session.turn_count === 0) {
    args.push("--session-id", session.id);
  } else {
    args.push("--resume", session.id);
  }
  args.push("--permission-mode", settings.agentPermissionMode);
  const extra = settings.agentClaudeArgs.trim();
  if (extra) {
    args.push(...extra.split(/\s+/));
  }
  args.push(prompt);
  return args;
}

/**
 * Spawn one detached turn for the worktree's managed session (creating the
 * session on first use). Throws if a turn is already in flight.
 */
export function startTurn(worktree: Worktree, prompt: string, settings: Settings): ManagedSession {
  let session = getManagedSession(worktree.id);
  if (!session) {
    session = createManagedSession(randomUUID(), worktree.id, worktree.path);
  }
  if (isTurnRunning(session)) {
    throw new Error("Agent is still working on the previous prompt");
  }

  if (!existsSync(SESSIONS_DIR)) {
    mkdirSync(SESSIONS_DIR, { recursive: true });
  }
  const logPath = sessionLogPath(session.id);
  appendFileSync(
    logPath,
    JSON.stringify({ type: "am-user-prompt", text: prompt, ts: new Date().toISOString() }) + "\n"
  );

  const args = buildTurnArgs(session, prompt, settings);
  log("info", "claude-session", `Starting turn for ${worktree.branch} (session ${session.id}, turn ${session.turn_count + 1})`);
  log("debug", "claude-session", `claude ${args.slice(0, -1).join(" ")} <prompt>`);

  // stdout+stderr go straight to the log file; detached so the turn survives
  // the TUI (and this process) exiting.
  const fd = openSync(logPath, "a");
  let child;
  try {
    child = spawn("claude", args, {
      cwd: worktree.path,
      detached: true,
      stdio: ["ignore", fd, fd],
    });
  } finally {
    closeSync(fd);
  }

  if (child.pid === undefined) {
    throw new Error("Failed to spawn claude — is it installed and in PATH?");
  }
  child.on("error", (err) => {
    log("warn", "claude-session", `Spawn error for session ${session!.id}: ${err}`);
    appendFileSync(
      logPath,
      JSON.stringify({ type: "am-error", text: `Failed to start claude: ${err}`, ts: new Date().toISOString() }) + "\n"
    );
    clearManagedSessionTurnPid(session!.id);
  });
  child.unref();

  recordManagedSessionTurn(session.id, child.pid, prompt);

  publishMessage({
    type: "managed-session-update",
    worktreeId: worktree.id,
    sessionId: session.id,
    state: "turn-started",
  }).catch(() => {});

  return getManagedSession(worktree.id)!;
}

/** SIGTERM the in-flight turn, if any. Returns true if a process was signalled. */
export function stopTurn(session: ManagedSession): boolean {
  if (!session.turn_pid || !isTurnRunning(session)) return false;
  try {
    process.kill(session.turn_pid, "SIGTERM");
    log("info", "claude-session", `Stopped turn pid ${session.turn_pid} for session ${session.id}`);
    return true;
  } catch (err) {
    log("warn", "claude-session", `Failed to stop turn pid ${session.turn_pid}: ${err}`);
    return false;
  }
}

const TOOL_SUMMARY_KEYS = ["command", "file_path", "pattern", "description", "prompt", "url", "query"] as const;
const TOOL_SUMMARY_MAX = 100;

function summarizeToolUse(name: string, input: Record<string, unknown> | undefined): string {
  let detail = "";
  if (input) {
    for (const key of TOOL_SUMMARY_KEYS) {
      const value = input[key];
      if (typeof value === "string" && value.trim()) {
        detail = value.trim().replace(/\s+/g, " ");
        break;
      }
    }
  }
  if (detail.length > TOOL_SUMMARY_MAX) {
    detail = detail.slice(0, TOOL_SUMMARY_MAX - 1) + "…";
  }
  return detail ? `${name}: ${detail}` : name;
}

/**
 * Parse one JSONL line of the session log into zero or more chat messages.
 * Unknown, informational, and partially-written lines yield nothing.
 */
export function parseTranscriptLine(line: string): ChatMessage[] {
  let event: Record<string, unknown>;
  try {
    event = JSON.parse(line);
  } catch {
    return []; // partial write or non-JSON stderr noise
  }
  if (!event || typeof event !== "object") return [];

  switch (event.type) {
    case "am-user-prompt":
      return [{ role: "user", text: String(event.text ?? ""), ts: typeof event.ts === "string" ? event.ts : undefined }];

    case "am-error":
      return [{ role: "error", text: String(event.text ?? ""), ts: typeof event.ts === "string" ? event.ts : undefined }];

    case "assistant": {
      const message = event.message as { content?: unknown } | undefined;
      const content = Array.isArray(message?.content) ? message.content : [];
      const messages: ChatMessage[] = [];
      for (const block of content as Array<Record<string, unknown>>) {
        if (block.type === "text" && typeof block.text === "string" && block.text.trim()) {
          messages.push({ role: "assistant", text: block.text });
        } else if (block.type === "tool_use" && typeof block.name === "string") {
          messages.push({
            role: "tool",
            text: summarizeToolUse(block.name, block.input as Record<string, unknown> | undefined),
          });
        }
      }
      return messages;
    }

    case "result": {
      if (event.is_error) {
        return [{ role: "error", text: String(event.result ?? event.subtype ?? "turn failed") }];
      }
      const durationMs = typeof event.duration_ms === "number" ? event.duration_ms : null;
      const cost = typeof event.total_cost_usd === "number" ? event.total_cost_usd : null;
      const parts = ["turn complete"];
      if (durationMs !== null) parts.push(`${Math.round(durationMs / 1000)}s`);
      if (cost !== null) parts.push(`$${cost.toFixed(2)}`);
      return [{ role: "system", text: parts.join(" · ") }];
    }

    default:
      // system/init, user (tool results), stream_event — not rendered
      return [];
  }
}

/** Parse the full session log into a transcript. Missing file → empty. */
export function parseTranscript(sessionId: string): ChatMessage[] {
  const logPath = sessionLogPath(sessionId);
  if (!existsSync(logPath)) return [];
  let raw: string;
  try {
    raw = readFileSync(logPath, "utf-8");
  } catch (err) {
    log("warn", "claude-session", `Failed to read session log ${logPath}: ${err}`);
    return [];
  }
  const messages: ChatMessage[] = [];
  for (const line of raw.split("\n")) {
    if (!line.trim()) continue;
    messages.push(...parseTranscriptLine(line));
  }
  return messages;
}
