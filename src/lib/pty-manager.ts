import * as pty from "node-pty";
import { log } from "./logger.js";

const MAX_BUFFER_LINES = 1000;

export interface AgentMessage {
  timestamp: number;
  role: "user" | "agent";
  content: string;
}

export interface PtyInstance {
  id: string;
  pty: pty.IPty;
  buffer: string[];
  cwd: string;
  role?: string;
  messages: AgentMessage[];
}

// Strip cursor movement / erase sequences but preserve SGR colors
const CSI_NON_SGR = /\x1b\[[\d;]*[ABCDEFGHJKSTfnsu]/g;
// Strip OSC sequences (e.g. terminal title changes)
const OSC_SEQ = /\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)/g;
// Strip other control sequences we don't want
const OTHER_CTRL = /\x1b[()][AB012]/g;

export function parsePtyOutput(raw: string): string[] {
  let cleaned = raw
    .replace(OSC_SEQ, "")
    .replace(CSI_NON_SGR, "")
    .replace(OTHER_CTRL, "");

  // Handle \r by treating it as a line overwrite when not followed by \n
  cleaned = cleaned.replace(/\r(?!\n)/g, "\n");

  return cleaned.split("\n");
}

let idCounter = 0;

export function spawnPty(
  cwd: string,
  cols: number,
  rows: number,
  initialPrompt?: string,
): PtyInstance {
  const id = `pty-${++idCounter}-${Date.now()}`;

  log("info", "pty", `Spawning PTY ${id} in ${cwd} (${cols}x${rows})`);

  const shell = process.env.SHELL || "/bin/zsh";
  const claudeArgs = initialPrompt
    ? ["claude", "--prompt", initialPrompt]
    : ["claude"];

  const ptyProcess = pty.spawn(shell, ["-c", claudeArgs.join(" ")], {
    name: "xterm-256color",
    cols,
    rows,
    cwd,
    env: { ...process.env } as Record<string, string>,
  });

  const instance: PtyInstance = {
    id,
    pty: ptyProcess,
    buffer: [],
    cwd,
    role: initialPrompt ? "custom" : undefined,
    messages: [],
  };

  return instance;
}

export function destroyPty(instance: PtyInstance): void {
  log("info", "pty", `Destroying PTY ${instance.id}`);
  try {
    instance.pty.kill();
  } catch (err) {
    log("warn", "pty", `Error killing PTY ${instance.id}: ${err}`);
  }
}

export function writeToPty(instance: PtyInstance, data: string): void {
  instance.pty.write(data);

  // Track user input (only printable + enter, not raw control sequences)
  if (data === "\r" || data === "\n") {
    // Enter pressed - don't add as separate message
  } else if (data.length > 0 && data.charCodeAt(0) >= 32) {
    // Accumulate user input into messages on newline boundaries
    const lastMsg = instance.messages[instance.messages.length - 1];
    if (lastMsg && lastMsg.role === "user" && Date.now() - lastMsg.timestamp < 2000) {
      lastMsg.content += data;
    } else {
      instance.messages.push({
        timestamp: Date.now(),
        role: "user",
        content: data,
      });
    }
  }
}

export function resizePty(instance: PtyInstance, cols: number, rows: number): void {
  try {
    instance.pty.resize(cols, rows);
  } catch {
    // PTY may have already exited
  }
}

export function appendToBuffer(instance: PtyInstance, data: string): void {
  const lines = parsePtyOutput(data);

  for (const line of lines) {
    if (instance.buffer.length > 0 && lines.indexOf(line) === 0) {
      // Append first fragment to the last buffer line
      instance.buffer[instance.buffer.length - 1] += line;
    } else {
      instance.buffer.push(line);
    }
  }

  // Trim buffer to max size
  if (instance.buffer.length > MAX_BUFFER_LINES) {
    instance.buffer.splice(0, instance.buffer.length - MAX_BUFFER_LINES);
  }

  // Best-effort agent message tracking: accumulate output as agent messages
  const content = data.replace(/\x1b\[[0-9;]*m/g, "").trim();
  if (content.length > 0) {
    const lastMsg = instance.messages[instance.messages.length - 1];
    if (lastMsg && lastMsg.role === "agent" && Date.now() - lastMsg.timestamp < 5000) {
      lastMsg.content += content;
      lastMsg.timestamp = Date.now();
    } else {
      instance.messages.push({
        timestamp: Date.now(),
        role: "agent",
        content,
      });
    }
  }
}
