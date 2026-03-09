import { useState, useEffect, useCallback, useRef } from "react";
import { getStandaloneSessions, pruneStaleStandaloneSessions } from "../lib/db.js";
import { isEffectivelyOpenStandalone } from "../lib/agent-utils.js";
import type { StandaloneSession } from "../lib/types.js";

export function useStandaloneSessions(pollingIntervalMs: number): {
  sessions: StandaloneSession[];
  refresh: () => void;
} {
  const [sessions, setSessions] = useState<StandaloneSession[]>([]);
  const prevFingerprintRef = useRef("");

  // Prune stale sessions on mount
  useEffect(() => {
    const pruned = pruneStaleStandaloneSessions();
    if (pruned > 0) {
      // silently cleaned up
    }
  }, []);

  const refresh = useCallback(() => {
    const all = getStandaloneSessions();
    const visible = all.filter(
      (s) => s.is_open === 1 || isEffectivelyOpenStandalone(s)
    );

    const fingerprint = JSON.stringify(
      visible.map((s) => ({
        id: s.id,
        path: s.path,
        status: s.status,
        is_open: s.is_open,
        session_id: s.session_id,
        last_response: s.last_response,
        transcript_summary: s.transcript_summary,
        updated_at: s.updated_at,
      }))
    );

    if (fingerprint !== prevFingerprintRef.current) {
      prevFingerprintRef.current = fingerprint;
      setSessions(visible);
    }
  }, []);

  // Initial fetch
  useEffect(() => {
    refresh();
  }, [refresh]);

  // Polling
  useEffect(() => {
    const timer = setInterval(refresh, pollingIntervalMs);
    return () => clearInterval(timer);
  }, [refresh, pollingIntervalMs]);

  return { sessions, refresh };
}
