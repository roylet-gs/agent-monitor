import { appendFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "fs";
import { dirname } from "path";
import { APP_DIR, LOG_PATH } from "./paths.js";

type LogLevel = "debug" | "info" | "warn" | "error";

const LEVEL_ORDER: Record<LogLevel, number> = {
  debug: 0,
  info: 1,
  warn: 2,
  error: 3,
};

const MAX_LINES = 5000;
const TRUNCATE_TO = 1000;

let currentLogLevel: LogLevel = "info";

export function setLogLevel(level: LogLevel): void {
  currentLogLevel = level;
}

function ensureLogDir(): void {
  if (!existsSync(APP_DIR)) {
    mkdirSync(APP_DIR, { recursive: true });
  }
}

function rotateIfNeeded(): void {
  if (!existsSync(LOG_PATH)) return;
  try {
    const content = readFileSync(LOG_PATH, "utf-8");
    const lines = content.split("\n");
    if (lines.length > MAX_LINES) {
      const truncated = lines.slice(-TRUNCATE_TO).join("\n");
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
  } catch {
    // ignore write errors
  }
}

export function initLogger(level: LogLevel): void {
  setLogLevel(level);
  ensureLogDir();
  rotateIfNeeded();
  log("info", "logger", "Logger initialized");
}
