/**
 * React hook that connects to the daemon for data updates.
 * Replaces useWorktrees + usePubSub + useStandaloneSessions when daemon is available.
 * Falls back to in-process polling (existing useWorktrees logic) if daemon is unavailable.
 */
import { useState, useEffect, useCallback, useRef } from "react";
import { DaemonClient } from "../lib/daemon-client.js";
import { useWorktrees, type WorktreeHookConfig } from "./useWorktrees.js";
import { useStandaloneSessions } from "./useStandaloneSessions.js";
import { usePubSub } from "./usePubSub.js";
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

const DAEMON_CONNECT_TIMEOUT_MS = 3000;

export function useDaemon(config: DaemonHookConfig): DaemonHookResult {
  const { repositories, settings, onAgentUpdate } = config;

  // --- Daemon mode state ---
  const [daemonData, setDaemonData] = useState<{
    groups: WorktreeGroup[];
    flatWorktrees: WorktreeWithStatus[];
    standaloneSessions: StandaloneSession[];
  }>({ groups: [], flatWorktrees: [], standaloneSessions: [] });

  const [connected, setConnected] = useState(false);
  const [fallbackMode, setFallbackMode] = useState(false);
  const clientRef = useRef<DaemonClient | null>(null);
  const prevFingerprintRef = useRef("");
  const onAgentUpdateRef = useRef(onAgentUpdate);
  onAgentUpdateRef.current = onAgentUpdate;

  // --- Fallback mode hooks (always called, but only used when fallbackMode=true) ---
  // When daemon is connected, disable fallback polling by passing empty repos and huge intervals
  const DISABLED_INTERVAL = 2_147_483_647; // max safe 32-bit int for setInterval

  const worktreeConfig: WorktreeHookConfig = {
    repositories: fallbackMode ? repositories : [],
    pollingIntervalMs: fallbackMode ? settings.pollingIntervalMs : DISABLED_INTERVAL,
    ghPollingIntervalMs: fallbackMode ? settings.ghPollingIntervalMs : DISABLED_INTERVAL,
    linearPollingIntervalMs: fallbackMode ? settings.linearPollingIntervalMs : DISABLED_INTERVAL,
    ghPrStatus: fallbackMode ? settings.ghPrStatus : false,
    linearEnabled: fallbackMode ? settings.linearEnabled : false,
    linearApiKey: settings.linearApiKey,
    hideMainBranch: settings.hideMainBranch,
    ghRefreshOnManual: settings.ghRefreshOnManual,
    linearRefreshOnManual: settings.linearRefreshOnManual,
    linearAutoNickname: settings.linearAutoNickname,
  };

  const fallbackWorktrees = useWorktrees(worktreeConfig);
  const fallbackStandalone = useStandaloneSessions(
    fallbackMode ? settings.pollingIntervalMs : DISABLED_INTERVAL
  );

  // Fallback pubsub refs
  const fallbackRefreshRef = useRef(fallbackWorktrees.refresh);
  useEffect(() => { fallbackRefreshRef.current = fallbackWorktrees.refresh; }, [fallbackWorktrees.refresh]);
  const fallbackLightRefreshRef = useRef(fallbackWorktrees.lightRefresh);
  useEffect(() => { fallbackLightRefreshRef.current = fallbackWorktrees.lightRefresh; }, [fallbackWorktrees.lightRefresh]);
  const fallbackStandaloneRefreshRef = useRef(fallbackStandalone.refresh);
  useEffect(() => { fallbackStandaloneRefreshRef.current = fallbackStandalone.refresh; }, [fallbackStandalone.refresh]);

  usePubSub(fallbackMode ? (msg) => {
    if (msg.type === "agent-status-update") {
      fallbackLightRefreshRef.current();
    } else if (msg.type === "standalone-status-update") {
      fallbackStandaloneRefreshRef.current();
    } else if (msg.type === "git-activity") {
      log("info", "useDaemon", `Git activity detected (fallback): ${msg.activity} on ${msg.branch}`);
      setTimeout(() => fallbackRefreshRef.current(), 3000);
    }
    onAgentUpdateRef.current?.(msg);
  } : () => {});

  // --- Daemon connection ---

  // Track settings changes to notify daemon
  const settingsRef = useRef(settings);
  useEffect(() => {
    const prev = settingsRef.current;
    settingsRef.current = settings;

    if (!fallbackMode && (
      prev.pollingIntervalMs !== settings.pollingIntervalMs ||
      prev.ghPollingIntervalMs !== settings.ghPollingIntervalMs ||
      prev.linearPollingIntervalMs !== settings.linearPollingIntervalMs ||
      prev.ghPrStatus !== settings.ghPrStatus ||
      prev.linearEnabled !== settings.linearEnabled ||
      prev.linearApiKey !== settings.linearApiKey ||
      prev.hideMainBranch !== settings.hideMainBranch ||
      prev.linearAutoNickname !== settings.linearAutoNickname
    )) {
      clientRef.current?.configReload();
    }
  }, [settings, fallbackMode]);

  // Notify daemon when repos change
  useEffect(() => {
    if (!fallbackMode) {
      clientRef.current?.configReload();
    }
  }, [repositories, fallbackMode]);

  useEffect(() => {
    let cancelled = false;

    const client = new DaemonClient({
      onData: (msg: DaemonToTuiMessage) => {
        if (cancelled) return;
        if (msg.type === "refresh-result") {
          const { data: newData } = msg;

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
            setDaemonData(newData);
          }
        } else if (msg.type === "agent-update") {
          onAgentUpdateRef.current?.(msg.original);
        }
      },
      onConnected: () => {
        if (!cancelled) {
          setConnected(true);
          setFallbackMode(false);
          log("info", "useDaemon", "Connected to daemon — disabling fallback polling");
        }
      },
      onDisconnected: () => {
        if (!cancelled) {
          setConnected(false);
          // Switch to fallback mode when daemon disconnects
          setFallbackMode(true);
          log("info", "useDaemon", "Daemon disconnected — enabling fallback polling");
        }
      },
    });

    clientRef.current = client;

    // Try to connect, fall back after timeout
    const connectTimeout = setTimeout(() => {
      if (!cancelled && !client.connected) {
        log("warn", "useDaemon", "Daemon connect timeout — using fallback polling");
        setFallbackMode(true);
      }
    }, DAEMON_CONNECT_TIMEOUT_MS);

    client.connect().then((ok) => {
      clearTimeout(connectTimeout);
      if (!cancelled && !ok) {
        log("warn", "useDaemon", "Could not connect to daemon — using fallback polling");
        setFallbackMode(true);
      }
    });

    return () => {
      cancelled = true;
      clearTimeout(connectTimeout);
      client.destroy();
      clientRef.current = null;
    };
  }, []);

  // --- Choose data source based on mode ---

  const refresh = useCallback(async () => {
    const client = clientRef.current;
    if (client?.connected) {
      await client.forceRefresh(true);
    } else {
      await fallbackRefreshRef.current();
    }
  }, []);

  const lightRefresh = useCallback(async () => {
    const client = clientRef.current;
    if (client?.connected) {
      await client.forceRefresh(false);
    } else {
      await fallbackLightRefreshRef.current();
    }
  }, []);

  // Use daemon data when connected, fallback data otherwise
  if (connected && !fallbackMode) {
    return {
      groups: daemonData.groups,
      flatWorktrees: daemonData.flatWorktrees,
      standaloneSessions: daemonData.standaloneSessions,
      refresh,
      lightRefresh,
      connected: true,
    };
  }

  return {
    groups: fallbackWorktrees.groups,
    flatWorktrees: fallbackWorktrees.flatWorktrees,
    standaloneSessions: fallbackStandalone.sessions,
    refresh,
    lightRefresh,
    connected: false,
  };
}
