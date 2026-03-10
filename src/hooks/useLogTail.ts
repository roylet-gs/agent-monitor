import { useState, useEffect, useRef } from "react";
import { existsSync, readFileSync } from "fs";
import { LOG_PATH } from "../lib/paths.js";

export function useLogTail(enabled: boolean, maxLines: number): string[] {
  const [lines, setLines] = useState<string[]>([]);
  const enabledRef = useRef(enabled);
  enabledRef.current = enabled;
  const prevContentRef = useRef<string>("");

  useEffect(() => {
    if (!enabled) {
      setLines([]);
      prevContentRef.current = "";
      return;
    }

    const poll = () => {
      if (!enabledRef.current) return;
      try {
        if (!existsSync(LOG_PATH)) {
          if (prevContentRef.current !== "") {
            prevContentRef.current = "";
            setLines([]);
          }
          return;
        }
        const content = readFileSync(LOG_PATH, "utf-8");
        const allLines = content.split("\n").filter((l) => l.length > 0);
        const sliced = allLines.slice(-maxLines);
        const joined = sliced.join("\n");
        if (joined !== prevContentRef.current) {
          prevContentRef.current = joined;
          setLines(sliced);
        }
      } catch {
        // file may be temporarily unavailable
      }
    };

    poll();
    const id = setInterval(poll, 2000);
    return () => clearInterval(id);
  }, [enabled, maxLines]);

  return lines;
}
