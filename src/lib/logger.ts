import { appendFileSync, existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from "fs";
import { APP_DIR, LOG_PATH } from "./paths.js";

type LogLevel = "debug" | "info" | "warn" | "error";

const LEVEL_ORDER: Record<LogLevel, number> = {
  debug: 0,
  info: 1,
  warn: 2,
  error: 3,
};

const TRUNCATE_TO_LINES = 1000;
const ROTATION_CHECK_INTERVAL = 100;

let currentLogLevel: LogLevel = "info";
let maxLogSizeBytes = 2 * 1024 * 1024; // default 2 MB
let writeCount = 0;

export function setLogLevel(level: LogLevel): void {
  currentLogLevel = level;
}

export function setMaxLogSize(mb: number): void {
  maxLogSizeBytes = Math.max(1, mb) * 1024 * 1024;
}

function ensureLogDir(): void {
  if (!existsSync(APP_DIR)) {
    mkdirSync(APP_DIR, { recursive: true });
  }
}

function rotateIfNeeded(): void {
  if (!existsSync(LOG_PATH)) return;
  try {
    const stats = statSync(LOG_PATH);
    if (stats.size > maxLogSizeBytes) {
      const content = readFileSync(LOG_PATH, "utf-8");
      const lines = content.split("\n");
      const truncated = lines.slice(-TRUNCATE_TO_LINES).join("\n");
      writeFileSync(LOG_PATH, truncated);
    }
  } catch {
    // ignore rotation errors
  }
}

export function log(level: LogLevel, module: string, message: string): void {
  if (LEVEL_ORDER[level] < LEVEL_ORDER[currentLogLevel]) return;
  ensureLogDir();
  const timestamp = new Date().toISOString();
  const line = `[${timestamp}] [${level.toUpperCase()}] [${module}] ${message}\n`;
  try {
    appendFileSync(LOG_PATH, line);
    writeCount++;
    if (writeCount >= ROTATION_CHECK_INTERVAL) {
      writeCount = 0;
      rotateIfNeeded();
    }
  } catch {
    // ignore write errors
  }
}

export function initLogger(level: LogLevel, version?: string, maxLogSizeMb?: number): void {
  setLogLevel(level);
  if (maxLogSizeMb !== undefined) {
    setMaxLogSize(maxLogSizeMb);
  }
  ensureLogDir();
  rotateIfNeeded();
  log("debug", "logger", `Logger initialized (v${version ?? "unknown"})`);
}
