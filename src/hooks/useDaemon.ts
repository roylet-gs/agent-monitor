/**
 * React hook that connects to the daemon for data updates.
 * Replaces useWorktrees + usePubSub + useStandaloneSessions when daemon is available.
 * Falls back to in-process polling (existing useWorktrees logic) if daemon is unavailable.
 */
import { useState, useEffect, useCallback, useRef } from "react";
import { DaemonClient } from "../lib/daemon-client.js";
import { log } from "../lib/logger.js";
import type { DaemonToTuiMessage } from "../lib/daemon-types.js";
import type { PubSubMessage } from "../lib/pubsub-types.js";
import type { WorktreeGroup, WorktreeWithStatus, StandaloneSession, Repository, Settings } from "../lib/types.js";

export interface DaemonHookConfig {
  repositories: Repository[];
  settings: Settings;
  onAgentUpdate?: (msg: PubSubMessage) => void;
}

export interface DaemonHookResult {
  groups: WorktreeGroup[];
  flatWorktrees: WorktreeWithStatus[];
  standaloneSessions: StandaloneSession[];
  refresh: () => Promise<void>;
  lightRefresh: () => Promise<void>;
  connected: boolean;
}

export function useDaemon(config: DaemonHookConfig): DaemonHookResult {
  const { repositories, settings, onAgentUpdate } = config;

  const [data, setData] = useState<{
    groups: WorktreeGroup[];
    flatWorktrees: WorktreeWithStatus[];
    standaloneSessions: StandaloneSession[];
  }>({ groups: [], flatWorktrees: [], standaloneSessions: [] });

  const [connected, setConnected] = useState(false);
  const clientRef = useRef<DaemonClient | null>(null);
  const prevFingerprintRef = useRef("");
  const onAgentUpdateRef = useRef(onAgentUpdate);
  onAgentUpdateRef.current = onAgentUpdate;

  // Track settings changes to notify daemon
  const settingsRef = useRef(settings);
  useEffect(() => {
    const prev = settingsRef.current;
    settingsRef.current = settings;

    // If polling intervals or integration settings changed, tell daemon to reload
    if (
      prev.pollingIntervalMs !== settings.pollingIntervalMs ||
      prev.ghPollingIntervalMs !== settings.ghPollingIntervalMs ||
      prev.linearPollingIntervalMs !== settings.linearPollingIntervalMs ||
      prev.ghPrStatus !== settings.ghPrStatus ||
      prev.linearEnabled !== settings.linearEnabled ||
      prev.linearApiKey !== settings.linearApiKey ||
      prev.hideMainBranch !== settings.hideMainBranch ||
      prev.linearAutoNickname !== settings.linearAutoNickname
    ) {
      clientRef.current?.configReload();
    }
  }, [settings]);

  // Notify daemon when repos change
  useEffect(() => {
    clientRef.current?.configReload();
  }, [repositories]);

  useEffect(() => {
    const client = new DaemonClient({
      onData: (msg: DaemonToTuiMessage) => {
        if (msg.type === "refresh-result") {
          const { data: newData } = msg;

          // Fingerprint to avoid unnecessary re-renders
          const fingerprint = JSON.stringify(newData.flatWorktrees.map(wt => ({
            id: wt.id, branch: wt.branch, custom_name: wt.custom_name, is_main: wt.is_main,
            status: wt.agent_status?.status,
            is_open: wt.agent_status?.is_open,
            summary: wt.agent_status?.transcript_summary,
            response: wt.agent_status?.last_response,
            ahead: wt.git_status?.ahead, behind: wt.git_status?.behind,
            dirty: wt.git_status?.dirty,
            commit_msg: wt.last_commit?.message, commit_time: wt.last_commit?.relative_time,
            has_terminal: wt.has_terminal, open_ide: wt.open_ide,
            pr: wt.pr_info?.number, pr_state: wt.pr_info?.state, checks: wt.pr_info?.checksStatus,
            active_check: wt.pr_info?.activeCheckUrl, checks_waiting: wt.pr_info?.checksWaiting,
            linear: wt.linear_info?.identifier, linear_state: wt.linear_info?.state?.type,
            linear_pr_url: wt.linear_info?.prAttachment?.url,
          }))) + JSON.stringify(newData.standaloneSessions.map(s => ({
            id: s.id, path: s.path, status: s.status, is_open: s.is_open,
            session_id: s.session_id, last_response: s.last_response,
            transcript_summary: s.transcript_summary, updated_at: s.updated_at,
          })));

          if (fingerprint !== prevFingerprintRef.current) {
            prevFingerprintRef.current = fingerprint;
            setData(newData);
          }
        } else if (msg.type === "agent-update") {
          onAgentUpdateRef.current?.(msg.original);
        }
      },
      onConnected: () => {
        setConnected(true);
      },
      onDisconnected: () => {
        setConnected(false);
      },
    });

    clientRef.current = client;
    client.connect().then((ok) => {
      if (!ok) {
        log("warn", "useDaemon", "Could not connect to daemon — will retry");
      }
    });

    return () => {
      client.destroy();
      clientRef.current = null;
    };
  }, []);

  const refresh = useCallback(async () => {
    const client = clientRef.current;
    if (client?.connected) {
      await client.forceRefresh(true);
    }
  }, []);

  const lightRefresh = useCallback(async () => {
    const client = clientRef.current;
    if (client?.connected) {
      await client.forceRefresh(false);
    }
  }, []);

  return {
    groups: data.groups,
    flatWorktrees: data.flatWorktrees,
    standaloneSessions: data.standaloneSessions,
    refresh,
    lightRefresh,
    connected,
  };
}
