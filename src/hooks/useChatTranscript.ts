import { useEffect, useRef, useState } from "react";
import { statSync } from "fs";
import { getManagedSession } from "../lib/db.js";
import { parseTranscript, sessionLogPath, isTurnRunning } from "../lib/claude-session.js";
import type { ChatMessage, ManagedSession } from "../lib/types.js";

interface ChatTranscript {
  session: ManagedSession | null;
  transcript: ChatMessage[];
  turnRunning: boolean;
}

/**
 * Live view of a worktree's managed session: polls the SQLite row (cheap,
 * synchronous) and re-parses the session's JSONL log whenever it grows.
 * The detached turn process writes the log, so polling the file is the
 * TUI's only reliable stream — it works even for turns started by the CLI
 * or a previous TUI instance.
 */
export function useChatTranscript(worktreeId: string, intervalMs = 500): ChatTranscript {
  const [session, setSession] = useState<ManagedSession | null>(null);
  const [transcript, setTranscript] = useState<ChatMessage[]>([]);
  const [turnRunning, setTurnRunning] = useState(false);
  const logSizeRef = useRef(-1);
  const sessionIdRef = useRef<string | null>(null);

  useEffect(() => {
    logSizeRef.current = -1;
    sessionIdRef.current = null;

    const tick = () => {
      const current = getManagedSession(worktreeId) ?? null;
      setSession((prev) =>
        prev?.id === current?.id &&
        prev?.turn_pid === current?.turn_pid &&
        prev?.turn_count === current?.turn_count
          ? prev
          : current
      );
      setTurnRunning(current ? isTurnRunning(current) : false);

      if (!current) {
        if (sessionIdRef.current !== null) {
          sessionIdRef.current = null;
          setTranscript([]);
        }
        return;
      }

      let size = 0;
      try {
        size = statSync(sessionLogPath(current.id)).size;
      } catch {
        size = 0;
      }
      if (current.id !== sessionIdRef.current || size !== logSizeRef.current) {
        sessionIdRef.current = current.id;
        logSizeRef.current = size;
        setTranscript(parseTranscript(current.id));
      }
    };

    tick();
    const timer = setInterval(tick, intervalMs);
    return () => clearInterval(timer);
  }, [worktreeId, intervalMs]);

  return { session, transcript, turnRunning };
}
