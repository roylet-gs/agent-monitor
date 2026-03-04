import { existsSync, readFileSync, truncateSync, watchFile, unwatchFile, statSync } from "fs";
import { LOG_PATH } from "../lib/paths.js";

interface LogsOptions {
  lines: number;
  follow: boolean;
  level?: string;
  module?: string;
  clear: boolean;
}

const LEVEL_HIERARCHY: Record<string, string[]> = {
  debug: ["DEBUG", "INFO", "WARN", "ERROR"],
  info: ["INFO", "WARN", "ERROR"],
  warn: ["WARN", "ERROR"],
  error: ["ERROR"],
};

function filterLines(lines: string[], level?: string, module?: string): string[] {
  let filtered = lines;

  if (level) {
    const allowed = LEVEL_HIERARCHY[level.toLowerCase()];
    if (allowed) {
      filtered = filtered.filter((line) =>
        allowed.some((l) => line.includes(`[${l}]`))
      );
    }
  }

  if (module) {
    const tag = `[${module}]`;
    filtered = filtered.filter((line) => line.includes(tag));
  }

  return filtered;
}

export function printLogs(options: LogsOptions): void {
  if (options.clear) {
    if (existsSync(LOG_PATH)) {
      truncateSync(LOG_PATH, 0);
      console.log("Log file cleared.");
    } else {
      console.log("No log file found.");
    }
    return;
  }

  if (!existsSync(LOG_PATH)) {
    console.log("No log file found at", LOG_PATH);
    return;
  }

  const content = readFileSync(LOG_PATH, "utf-8");
  const allLines = content.split("\n").filter((l) => l.length > 0);
  const filtered = filterLines(allLines, options.level, options.module);
  const tail = filtered.slice(-options.lines);

  for (const line of tail) {
    console.log(line);
  }

  if (!options.follow) return;

  // Follow mode: watch for changes and print new lines
  let lastSize = statSync(LOG_PATH).size;

  console.log("--- following log (Ctrl+C to stop) ---");

  watchFile(LOG_PATH, { interval: 300 }, () => {
    try {
      const stats = statSync(LOG_PATH);
      if (stats.size <= lastSize) {
        lastSize = stats.size;
        return;
      }

      const fd = require("fs").openSync(LOG_PATH, "r");
      const buf = Buffer.alloc(stats.size - lastSize);
      require("fs").readSync(fd, buf, 0, buf.length, lastSize);
      require("fs").closeSync(fd);
      lastSize = stats.size;

      const newLines = buf.toString("utf-8").split("\n").filter((l: string) => l.length > 0);
      const newFiltered = filterLines(newLines, options.level, options.module);
      for (const line of newFiltered) {
        console.log(line);
      }
    } catch {
      // file may have been truncated
      lastSize = 0;
    }
  });

  // Keep process alive and clean up on exit
  process.on("SIGINT", () => {
    unwatchFile(LOG_PATH);
    process.exit(0);
  });
}
