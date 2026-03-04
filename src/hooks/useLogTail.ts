import { useState, useEffect, useRef } from "react";
import { existsSync, readFileSync } from "fs";
import { LOG_PATH } from "../lib/paths.js";

export function useLogTail(enabled: boolean, maxLines: number): string[] {
  const [lines, setLines] = useState<string[]>([]);
  const enabledRef = useRef(enabled);
  enabledRef.current = enabled;

  useEffect(() => {
    if (!enabled) {
      setLines([]);
      return;
    }

    const poll = () => {
      if (!enabledRef.current) return;
      try {
        if (!existsSync(LOG_PATH)) {
          setLines([]);
          return;
        }
        const content = readFileSync(LOG_PATH, "utf-8");
        const allLines = content.split("\n").filter((l) => l.length > 0);
        setLines(allLines.slice(-maxLines));
      } catch {
        // file may be temporarily unavailable
      }
    };

    poll();
    const id = setInterval(poll, 500);
    return () => clearInterval(id);
  }, [enabled, maxLines]);

  return lines;
}
