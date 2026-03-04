import { appendFileSync, existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from "fs";
import { randomUUID } from "crypto";
import { APP_DIR, LOG_PATH } from "./paths.js";

type LogLevel = "debug" | "info" | "warn" | "error";

export interface LogContext {
  correlationId?: string;
  worktreeId?: string;
  sessionId?: string;
  event?: string;
  durationMs?: number;
  [key: string]: string | number | boolean | undefined;
}

const LEVEL_ORDER: Record<LogLevel, number> = {
  debug: 0,
  info: 1,
  warn: 2,
  error: 3,
};

const MAX_LINES = 5000;
const TRUNCATE_TO = 1000;
const MAX_SIZE_BYTES = 5 * 1024 * 1024; // 5MB

let currentLogLevel: LogLevel = "info";

export function setLogLevel(level: LogLevel): void {
  currentLogLevel = level;
}

export function generateCorrelationId(): string {
  return randomUUID().slice(0, 8);
}

function ensureLogDir(): void {
  if (!existsSync(APP_DIR)) {
    mkdirSync(APP_DIR, { recursive: true });
  }
}

function rotateIfNeeded(): void {
  if (!existsSync(LOG_PATH)) return;
  try {
    // Check file size first (cheaper than reading content)
    const stats = statSync(LOG_PATH);
    if (stats.size > MAX_SIZE_BYTES) {
      const content = readFileSync(LOG_PATH, "utf-8");
      const lines = content.split("\n");
      const truncated = lines.slice(-TRUNCATE_TO).join("\n");
      writeFileSync(LOG_PATH, truncated);
      return;
    }

    const content = readFileSync(LOG_PATH, "utf-8");
    const lines = content.split("\n");
    if (lines.length > MAX_LINES) {
      const truncated = lines.slice(-TRUNCATE_TO).join("\n");
      writeFileSync(LOG_PATH, truncated);
    }
  } catch (err) {
    // Log rotation failure to stderr so it's not completely lost
    process.stderr.write(`[agent-monitor] Log rotation failed: ${err}\n`);
  }
}

function formatContext(ctx: LogContext): string {
  const parts: string[] = [];
  for (const [key, value] of Object.entries(ctx)) {
    if (value !== undefined) {
      parts.push(`${key}=${value}`);
    }
  }
  return parts.length > 0 ? ` | ${parts.join(" ")}` : "";
}

export function log(level: LogLevel, module: string, message: string, ctx?: LogContext): void {
  if (LEVEL_ORDER[level] < LEVEL_ORDER[currentLogLevel]) return;
  ensureLogDir();
  const timestamp = new Date().toISOString();
  const ctxStr = ctx ? formatContext(ctx) : "";
  const line = `[${timestamp}] [${level.toUpperCase()}] [${module}] ${message}${ctxStr}\n`;
  try {
    appendFileSync(LOG_PATH, line);
  } catch {
    // ignore write errors
  }
}

export async function timeOperation<T>(
  level: LogLevel,
  module: string,
  label: string,
  fn: () => Promise<T>,
  ctx?: LogContext
): Promise<T> {
  const start = performance.now();
  try {
    const result = await fn();
    const durationMs = Math.round(performance.now() - start);
    log(level, module, `${label} completed`, { ...ctx, durationMs });
    return result;
  } catch (err) {
    const durationMs = Math.round(performance.now() - start);
    log("error", module, `${label} failed: ${err}`, { ...ctx, durationMs });
    throw err;
  }
}

export function timeOperationSync<T>(
  level: LogLevel,
  module: string,
  label: string,
  fn: () => T,
  ctx?: LogContext
): T {
  const start = performance.now();
  try {
    const result = fn();
    const durationMs = Math.round(performance.now() - start);
    log(level, module, `${label} completed`, { ...ctx, durationMs });
    return result;
  } catch (err) {
    const durationMs = Math.round(performance.now() - start);
    log("error", module, `${label} failed: ${err}`, { ...ctx, durationMs });
    throw err;
  }
}

export function initLogger(level: LogLevel): void {
  setLogLevel(level);
  ensureLogDir();
  rotateIfNeeded();
  log("info", "logger", "Logger initialized");
}
