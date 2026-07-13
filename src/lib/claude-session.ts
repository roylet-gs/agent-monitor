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
import { appendFileSync, existsSync, mkdirSync, openSync, closeSync, readFileSync, readdirSync, statSync } from "fs";
import { join } from "path";
import { homedir } from "os";
import { randomUUID } from "crypto";
import { SESSIONS_DIR } from "./paths.js";
import {
  createManagedSession,
  replaceManagedSession,
  getManagedSession,
  getAgentStatus,
  recordManagedSessionTurn,
  clearManagedSessionTurnPid,
} from "./db.js";
import { publishMessage } from "./pubsub-client.js";
import { log } from "./logger.js";
import type { ChatMessage, ManagedSession, Settings, Worktree } from "./types.js";

export function sessionLogPath(sessionId: string): string {
  return join(SESSIONS_DIR, `${sessionId}.jsonl`);
}

/** Claude Code encodes a project cwd with every non-alphanumeric char as "-". */
export function encodeProjectPath(cwd: string): string {
  return cwd.replace(/[^a-zA-Z0-9]/g, "-");
}

function claudeProjectsDir(): string {
  return join(homedir(), ".claude", "projects");
}

/**
 * Claude Code's own transcript for a session started exactly at cwd:
 * ~/.claude/projects/<encoded cwd>/<id>.jsonl.
 */
export function claudeTranscriptPath(cwd: string, sessionId: string): string {
  return join(claudeProjectsDir(), encodeProjectPath(cwd), `${sessionId}.jsonl`);
}

/**
 * Locate a session's transcript for a session started at rootPath OR any
 * subdirectory of it (each start dir gets its own encoded project dir).
 * Session ids are UUIDs, so a filename match is unambiguous.
 */
export function findClaudeTranscript(rootPath: string, sessionId: string): string | null {
  const exact = claudeTranscriptPath(rootPath, sessionId);
  if (existsSync(exact)) return exact;
  for (const dir of candidateProjectDirs(rootPath)) {
    const file = join(dir, `${sessionId}.jsonl`);
    if (existsSync(file)) return file;
  }
  return null;
}

/**
 * Project dirs that may belong to rootPath or a subdirectory of it.
 * Prefix matching over the encoded name is a superset (e.g. "/wt.bak"
 * encodes like a "/wt" subdir), so callers must verify via the cwd field
 * inside the transcript when it matters.
 */
function candidateProjectDirs(rootPath: string): string[] {
  const prefix = encodeProjectPath(rootPath);
  let names: string[];
  try {
    names = readdirSync(claudeProjectsDir());
  } catch {
    return [];
  }
  return names
    .filter((name) => name === prefix || name.startsWith(prefix + "-"))
    .map((name) => join(claudeProjectsDir(), name));
}

/** First cwd recorded inside a transcript file (claude stamps it on most lines). */
export function readTranscriptCwd(file: string): string | null {
  try {
    const raw = readFileSync(file, "utf-8");
    for (const line of raw.split("\n").slice(0, 50)) {
      if (!line.trim()) continue;
      try {
        const event = JSON.parse(line);
        if (typeof event?.cwd === "string" && event.cwd) return event.cwd;
      } catch {
        continue;
      }
    }
  } catch {
    return null;
  }
  return null;
}

export interface DiscoveredSession {
  id: string;
  /** Directory the session was started in (worktree root or a subdirectory). */
  cwd: string;
  file: string;
  mtimeMs: number;
  lastPrompt: string | null;
}

/**
 * All Claude sessions started at a worktree or in any of its
 * subdirectories, newest first. Candidate project dirs are matched by
 * encoded-name prefix and confirmed via the cwd recorded in each
 * transcript, so encoding collisions (e.g. a sibling "/wt-bak") are
 * filtered out.
 */
export function discoverWorktreeSessions(worktreePath: string): DiscoveredSession[] {
  const sessions: DiscoveredSession[] = [];
  for (const dir of candidateProjectDirs(worktreePath)) {
    let files: string[];
    try {
      files = readdirSync(dir).filter((f) => f.endsWith(".jsonl"));
    } catch {
      continue;
    }
    for (const name of files) {
      const file = join(dir, name);
      const cwd = readTranscriptCwd(file);
      if (!cwd || (cwd !== worktreePath && !cwd.startsWith(worktreePath + "/"))) continue;
      let mtimeMs = 0;
      try {
        mtimeMs = statSync(file).mtimeMs;
      } catch {
        continue;
      }
      sessions.push({
        id: name.slice(0, -".jsonl".length),
        cwd,
        file,
        mtimeMs,
        lastPrompt: readLastPrompt(file),
      });
    }
  }
  sessions.sort((a, b) => b.mtimeMs - a.mtimeMs);
  log("debug", "claude-session", `Discovered ${sessions.length} session(s) for ${worktreePath}`);
  return sessions;
}

/** Last real user prompt in a transcript, for picker labels. */
function readLastPrompt(file: string): string | null {
  try {
    const raw = readFileSync(file, "utf-8");
    const lines = raw.split("\n");
    for (let i = lines.length - 1; i >= 0; i--) {
      if (!lines[i]!.trim()) continue;
      const messages = parseClaudeTranscriptLine(lines[i]!);
      const user = messages.find((m) => m.role === "user");
      if (user) return user.text;
    }
  } catch {
    return null;
  }
  return null;
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
export function startTurn(
  worktree: Worktree,
  prompt: string,
  settings: Settings,
  sessionId?: string
): ManagedSession {
  let session = getManagedSession(worktree.id);
  if (session && isTurnRunning(session)) {
    throw new Error("Agent is still working on the previous prompt");
  }

  if (sessionId && sessionId !== session?.id) {
    // The user picked a specific session (e.g. one of several under this
    // worktree) — it becomes the worktree's active managed session and the
    // turn resumes it from its original start directory.
    const file = findClaudeTranscript(worktree.path, sessionId);
    const cwd = (file && readTranscriptCwd(file)) || worktree.path;
    log("info", "claude-session", `Switching managed session for ${worktree.branch} to ${sessionId} (cwd ${cwd})`);
    session = replaceManagedSession(worktree.id, sessionId, cwd, 1);
  } else if (!session) {
    // Adopt a session started outside am (e.g. interactive claude in a
    // terminal) when hooks recorded its id and its transcript still exists —
    // the first managed turn then resumes that conversation.
    const externalId = getAgentStatus(worktree.id)?.session_id;
    const externalFile = externalId ? findClaudeTranscript(worktree.path, externalId) : null;
    if (externalId && externalFile) {
      const cwd = readTranscriptCwd(externalFile) ?? worktree.path;
      log("info", "claude-session", `Adopting external session ${externalId} for ${worktree.branch} (cwd ${cwd})`);
      session = createManagedSession(externalId, worktree.id, cwd, 1);
    } else {
      session = createManagedSession(randomUUID(), worktree.id, worktree.path);
    }
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
  // the TUI (and this process) exiting. Spawn from the session's original
  // start directory — claude resolves --resume within the current project.
  const fd = openSync(logPath, "a");
  let child;
  try {
    child = spawn("claude", args, {
      cwd: session.cwd,
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

/**
 * Parse one line of a Claude Code project transcript (~/.claude/projects).
 * The shape differs from -p stream-json: entries carry a full API message
 * plus metadata (isMeta for injected context, isSidechain for subagents).
 */
export function parseClaudeTranscriptLine(line: string): ChatMessage[] {
  let event: Record<string, unknown>;
  try {
    event = JSON.parse(line);
  } catch {
    return [];
  }
  if (!event || typeof event !== "object") return [];
  if (event.isMeta || event.isSidechain) return [];

  const ts = typeof event.timestamp === "string" ? event.timestamp : undefined;
  const message = event.message as { content?: unknown } | undefined;

  if (event.type === "user") {
    const content = message?.content;
    const texts: string[] = [];
    if (typeof content === "string") {
      texts.push(content);
    } else if (Array.isArray(content)) {
      for (const block of content as Array<Record<string, unknown>>) {
        if (block.type === "text" && typeof block.text === "string") texts.push(block.text);
        // tool_result blocks are tool plumbing, not user speech
      }
    }
    return texts
      .map((t) => t.trim())
      // skip injected wrappers (command messages, system reminders, caveats)
      .filter((t) => t && !t.startsWith("<") && !t.startsWith("Caveat:"))
      .map((text) => ({ role: "user" as const, text, ts }));
  }

  if (event.type === "assistant") {
    const content = Array.isArray(message?.content) ? message.content : [];
    const messages: ChatMessage[] = [];
    for (const block of content as Array<Record<string, unknown>>) {
      if (block.type === "text" && typeof block.text === "string" && block.text.trim()) {
        messages.push({ role: "assistant", text: block.text, ts });
      } else if (block.type === "tool_use" && typeof block.name === "string") {
        messages.push({
          role: "tool",
          text: summarizeToolUse(block.name, block.input as Record<string, unknown> | undefined),
          ts,
        });
      }
    }
    return messages;
  }

  return [];
}

/**
 * Load the best available transcript for a session at a given cwd.
 * Prefers Claude Code's own project transcript — it covers interactive
 * turns (attach, sessions started outside am) that am's stream-json log
 * never sees — and falls back to am's per-session log.
 */
export function loadTranscript(rootPath: string, sessionId: string): ChatMessage[] {
  const claudeFile = findClaudeTranscript(rootPath, sessionId);
  if (claudeFile) {
    try {
      const raw = readFileSync(claudeFile, "utf-8");
      const messages: ChatMessage[] = [];
      for (const line of raw.split("\n")) {
        if (!line.trim()) continue;
        messages.push(...parseClaudeTranscriptLine(line));
      }
      return messages;
    } catch (err) {
      log("warn", "claude-session", `Failed to read claude transcript ${claudeFile}: ${err}`);
    }
  }
  return parseTranscript(sessionId);
}

/** Path whose growth signals new transcript content (for cheap polling). */
export function transcriptWatchPath(rootPath: string, sessionId: string): string {
  return findClaudeTranscript(rootPath, sessionId) ?? sessionLogPath(sessionId);
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
