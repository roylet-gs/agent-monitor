import { useEffect, useRef, useState } from "react";
import { statSync } from "fs";
import { getManagedSession, getAgentStatus } from "../lib/db.js";
import { loadTranscript, transcriptWatchPath, isTurnRunning } from "../lib/claude-session.js";
import type { ChatMessage, ManagedSession } from "../lib/types.js";

interface ChatTranscript {
  session: ManagedSession | null;
  /** Effective session id: the managed session, or one hooks observed at this worktree. */
  sessionId: string | null;
  transcript: ChatMessage[];
  turnRunning: boolean;
}

/**
 * Live view of a worktree's Claude conversation: polls the SQLite rows
 * (cheap, synchronous) and re-parses the transcript whenever it grows.
 * The transcript source is Claude Code's own project file when it exists
 * (covers sessions started outside am and interactive attach turns), with
 * am's per-session stream-json log as fallback — see loadTranscript().
 */
export function useChatTranscript(
  worktreeId: string,
  cwd: string,
  overrideSessionId: string | null = null,
  intervalMs = 500
): ChatTranscript {
  const [session, setSession] = useState<ManagedSession | null>(null);
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [transcript, setTranscript] = useState<ChatMessage[]>([]);
  const [turnRunning, setTurnRunning] = useState(false);
  const fingerprintRef = useRef("");

  useEffect(() => {
    fingerprintRef.current = "";

    const tick = () => {
      const managed = getManagedSession(worktreeId) ?? null;
      // A user-picked session wins; otherwise the managed session, then a
      // session hooks observed at this worktree (started manually in a
      // terminal/IDE) so its history is visible before am ever prompts.
      const effectiveId = overrideSessionId ?? managed?.id ?? getAgentStatus(worktreeId)?.session_id ?? null;

      setSession((prev) =>
        prev?.id === managed?.id &&
        prev?.turn_pid === managed?.turn_pid &&
        prev?.turn_count === managed?.turn_count
          ? prev
          : managed
      );
      setSessionId(effectiveId);
      setTurnRunning(managed ? isTurnRunning(managed) : false);

      if (!effectiveId) {
        if (fingerprintRef.current !== "") {
          fingerprintRef.current = "";
          setTranscript([]);
        }
        return;
      }

      const watchPath = transcriptWatchPath(cwd, effectiveId);
      let size = 0;
      try {
        size = statSync(watchPath).size;
      } catch {
        size = 0;
      }
      const fingerprint = `${effectiveId}:${watchPath}:${size}`;
      if (fingerprint !== fingerprintRef.current) {
        fingerprintRef.current = fingerprint;
        setTranscript(loadTranscript(cwd, effectiveId));
      }
    };

    tick();
    const timer = setInterval(tick, intervalMs);
    return () => clearInterval(timer);
  }, [worktreeId, cwd, overrideSessionId, intervalMs]);

  return { session, sessionId, transcript, turnRunning };
}
