import { useState, useEffect, useCallback, useRef } from "react";
import { getAgentSessions, getDb } from "../lib/db.js";
import { log } from "../lib/logger.js";
import type { AgentSession } from "../lib/types.js";

export interface CompletedWork {
  roleName: string | null;
  summary: string;
  timestamp: string;
}

export function useManagedSessions(worktreeId: string): {
  sessions: AgentSession[];
  completedWork: CompletedWork[];
  refresh: () => void;
} {
  const [sessions, setSessions] = useState<AgentSession[]>([]);
  const [completedWork, setCompletedWork] = useState<CompletedWork[]>([]);
  const prevStatusRef = useRef<Map<string, string>>(new Map());

  const refresh = useCallback(() => {
    const rows = getAgentSessions(worktreeId);

    // PID liveness check
    for (const s of rows) {
      if (s.pid != null) {
        try {
          process.kill(s.pid, 0);
        } catch {
          getDb()
            .prepare("UPDATE agent_sessions SET pid = NULL, is_open = 0 WHERE id = ?")
            .run(s.id);
          s.pid = null;
          s.is_open = 0;
        }
      }
    }

    // Track transitions to "done" for completed work
    const prev = prevStatusRef.current;
    for (const s of rows) {
      const prevStatus = prev.get(s.id);
      if (prevStatus && prevStatus !== "done" && s.status === "done") {
        const summary = s.transcript_summary ?? s.last_response ?? "Completed";
        setCompletedWork((cw) => [
          { roleName: s.role_name, summary, timestamp: s.updated_at },
          ...cw,
        ]);
        log("info", "useManagedSessions", `Session ${s.id} completed: ${summary.slice(0, 80)}`);
      }
      prev.set(s.id, s.status);
    }

    setSessions(rows);
  }, [worktreeId]);

  useEffect(() => {
    refresh();
    const timer = setInterval(refresh, 2000);
    return () => clearInterval(timer);
  }, [refresh]);

  return { sessions, completedWork, refresh };
}
