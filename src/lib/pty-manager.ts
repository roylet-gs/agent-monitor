import { chmodSync, existsSync, readdirSync } from "fs";
import { dirname, join } from "path";
import { createRequire } from "module";
import * as pty from "node-pty";
import { createRequire as _createRequire } from "module";
const _require = _createRequire(import.meta.url);
const { Terminal: XTerminal } = _require("@xterm/headless") as { Terminal: typeof import("@xterm/headless").Terminal };
import { log } from "./logger.js";

export interface AgentMessage {
  timestamp: number;
  role: "user" | "agent";
  content: string;
}

export interface PtyInstance {
  id: string;
  pty: pty.IPty;
  terminal: InstanceType<typeof XTerminal>;
  cwd: string;
  role?: string;
  messages: AgentMessage[];
}

let idCounter = 0;
let spawnHelperFixed = false;

function ensureSpawnHelperPerms(): void {
  if (spawnHelperFixed) return;
  spawnHelperFixed = true;
  try {
    const require = createRequire(import.meta.url);
    const ptyMain = require.resolve("node-pty");
    // require.resolve returns .../lib/index.js, prebuilds is at package root
    const prebuildsDir = join(dirname(ptyMain), "..", "prebuilds");
    if (existsSync(prebuildsDir)) {
      for (const platform of readdirSync(prebuildsDir)) {
        const helper = join(prebuildsDir, platform, "spawn-helper");
        if (existsSync(helper)) {
          chmodSync(helper, 0o755);
        }
      }
    }
  } catch {
    // Best effort
  }
}

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

  // Ensure spawn-helper has execute permission (pnpm prebuilds may lose it)
  ensureSpawnHelperPerms();

  let ptyProcess: pty.IPty;
  try {
    ptyProcess = pty.spawn(shell, ["-c", claudeArgs.join(" ")], {
      name: "xterm-256color",
      cols,
      rows,
      cwd,
      env: { ...process.env } as Record<string, string>,
    });
  } catch (err) {
    log("error", "pty", `Failed to spawn PTY: ${err}. Try running: find node_modules -path '*/node-pty/prebuilds/*/spawn-helper' -exec chmod +x {} +`);
    throw new Error(
      `Failed to spawn terminal. Run: find node_modules -path '*/node-pty/prebuilds/*/spawn-helper' -exec chmod +x {} +`,
    );
  }

  const terminal = new XTerminal({ cols, rows, allowProposedApi: true });

  const instance: PtyInstance = {
    id,
    pty: ptyProcess,
    terminal,
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
  try {
    instance.terminal.dispose();
  } catch {
    // Terminal may already be disposed
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
  try {
    instance.terminal.resize(cols, rows);
  } catch {
    // Terminal may already be disposed
  }
}

export function writeToTerminal(instance: PtyInstance, data: string): void {
  instance.terminal.write(data);

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

function buildSgr(
  fg: number,
  fgMode: number,
  bg: number,
  bgMode: number,
  attrs: number,
): string {
  // fgMode/bgMode: 0=default, 1=palette, 2=rgb
  const parts: number[] = [];

  // Attributes: bold=1, dim=2, italic=4, underline=8, inverse=16, strikethrough=32
  if (attrs & 1) parts.push(1);
  if (attrs & 2) parts.push(2);
  if (attrs & 4) parts.push(3);
  if (attrs & 8) parts.push(4);
  if (attrs & 16) parts.push(7);
  if (attrs & 32) parts.push(9);

  // Foreground
  if (fgMode === 1) {
    if (fg < 8) parts.push(30 + fg);
    else if (fg < 16) parts.push(90 + fg - 8);
    else { parts.push(38, 5, fg); }
  } else if (fgMode === 2) {
    parts.push(38, 2, (fg >> 16) & 0xff, (fg >> 8) & 0xff, fg & 0xff);
  }

  // Background
  if (bgMode === 1) {
    if (bg < 8) parts.push(40 + bg);
    else if (bg < 16) parts.push(100 + bg - 8);
    else { parts.push(48, 5, bg); }
  } else if (bgMode === 2) {
    parts.push(48, 2, (bg >> 16) & 0xff, (bg >> 8) & 0xff, bg & 0xff);
  }

  if (parts.length === 0) return "";
  return `\x1b[${parts.join(";")}m`;
}

function serializeLineWithStyles(
  line: ReturnType<InstanceType<typeof XTerminal>["buffer"]["active"]["getLine"]>,
  cols: number,
  nullCell: ReturnType<NonNullable<ReturnType<InstanceType<typeof XTerminal>["buffer"]["active"]["getLine"]>>["getCell"]>,
): string {
  if (!line) return "";

  let result = "";
  let prevFg = -1;
  let prevFgMode = 0;
  let prevBg = -1;
  let prevBgMode = 0;
  let prevAttrs = 0;
  let lastNonSpaceIdx = -1;

  // First pass: find last non-space cell for trimming
  for (let col = cols - 1; col >= 0; col--) {
    line.getCell(col, nullCell);
    const ch = nullCell!.getChars();
    if (ch !== "" && ch !== " ") {
      lastNonSpaceIdx = col;
      break;
    }
  }

  // Second pass: build styled string
  for (let col = 0; col <= lastNonSpaceIdx; col++) {
    line.getCell(col, nullCell);

    const fg = nullCell!.isFgDefault() ? -1 : nullCell!.getFgColor();
    const fgMode = nullCell!.isFgDefault() ? 0 : nullCell!.isFgRGB() ? 2 : 1;
    const bg = nullCell!.isBgDefault() ? -1 : nullCell!.getBgColor();
    const bgMode = nullCell!.isBgDefault() ? 0 : nullCell!.isBgRGB() ? 2 : 1;

    let attrs = 0;
    if (nullCell!.isBold()) attrs |= 1;
    if (nullCell!.isDim()) attrs |= 2;
    if (nullCell!.isItalic()) attrs |= 4;
    if (nullCell!.isUnderline()) attrs |= 8;
    if (nullCell!.isInverse()) attrs |= 16;
    if (nullCell!.isStrikethrough()) attrs |= 32;

    if (fg !== prevFg || fgMode !== prevFgMode || bg !== prevBg || bgMode !== prevBgMode || attrs !== prevAttrs) {
      // Style changed — reset and apply new style
      const sgr = buildSgr(fg, fgMode, bg, bgMode, attrs);
      if (prevFg !== -1 || prevBg !== -1 || prevAttrs !== 0) {
        result += "\x1b[0m";
      }
      result += sgr;
      prevFg = fg;
      prevFgMode = fgMode;
      prevBg = bg;
      prevBgMode = bgMode;
      prevAttrs = attrs;
    }

    result += nullCell!.getChars() || " ";
  }

  // Reset at end if any style was applied
  if (prevFg !== -1 || prevBg !== -1 || prevAttrs !== 0) {
    result += "\x1b[0m";
  }

  return result;
}

export function getScreenLines(instance: PtyInstance): string[] {
  const buf = instance.terminal.buffer.active;
  const nullCell = buf.getNullCell();
  const lines: string[] = [];
  for (let row = 0; row < instance.terminal.rows; row++) {
    const line = buf.getLine(buf.viewportY + row);
    lines.push(line ? serializeLineWithStyles(line, instance.terminal.cols, nullCell) : "");
  }
  return lines;
}
