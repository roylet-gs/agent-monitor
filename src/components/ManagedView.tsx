import React, { useState, useCallback, useEffect } from "react";
import { Box, Text, useInput } from "ink";
import { RoleSelector } from "./RoleSelector.js";
import { useManagedSessions, type CompletedWork } from "../hooks/useManagedSessions.js";
import { createAgentSession, updateAgentSessionPid, removeAgentSession } from "../lib/db.js";
import { launchClaudeSession, openClaudeInTerminal, killClaudeSession } from "../lib/ide-launcher.js";
import { getRoleContent } from "../lib/roles.js";
import { log } from "../lib/logger.js";
import type { WorktreeWithStatus, AgentSession } from "../lib/types.js";

interface ManagedViewProps {
  worktree: WorktreeWithStatus;
  onBack: () => void;
  onError: (msg: string) => void;
  refreshTick: number;
}

function statusDot(status: string, isOpen: number): string {
  if (!isOpen && status !== "done") return "○";
  return "●";
}

function statusColor(status: string): string {
  switch (status) {
    case "executing": return "green";
    case "planning": return "cyan";
    case "waiting": return "yellow";
    case "done": return "blueBright";
    default: return "gray";
  }
}

function statusLabel(status: string): string {
  switch (status) {
    case "executing": return "Executing";
    case "planning": return "Planning";
    case "waiting": return "Waiting";
    case "done": return "Done";
    default: return "Idle";
  }
}

function timeAgo(dateStr: string): string {
  const ms = Date.now() - new Date(dateStr + "Z").getTime();
  const mins = Math.floor(ms / 60000);
  if (mins < 1) return "just now";
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.floor(mins / 60);
  return `${hours}h ago`;
}

export function ManagedView({ worktree, onBack, onError, refreshTick }: ManagedViewProps) {
  const { sessions, completedWork, refresh } = useManagedSessions(worktree.id);
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [subMode, setSubMode] = useState<"list" | "role-select">("list");
  const [launching, setLaunching] = useState(false);

  // Refresh when pub/sub tick changes (instant update from hook events)
  useEffect(() => {
    refresh();
  }, [refreshTick]);

  const selected = sessions[selectedIndex] ?? null;

  const handleLaunch = useCallback(async (roleName: string | null, roleContent: string | null) => {
    setSubMode("list");
    setLaunching(true);
    try {
      const session = createAgentSession(worktree.id, roleName);
      refresh(); // Show the new session immediately before PID discovery
      const pid = await launchClaudeSession(worktree.path, {
        continueSession: false,
        prompt: roleContent ?? undefined,
      });
      if (pid != null) {
        updateAgentSessionPid(session.id, pid);
      }
      log("info", "ManagedView", `Launched session ${session.id} role=${roleName ?? "none"} pid=${pid}`);
      refresh();
    } catch (err) {
      onError(`Failed to launch: ${err}`);
    } finally {
      setLaunching(false);
    }
  }, [worktree, refresh, onError]);

  const handleResume = useCallback(async (session: AgentSession) => {
    setLaunching(true);
    try {
      const pid = await launchClaudeSession(worktree.path, { continueSession: true });
      if (pid != null) {
        updateAgentSessionPid(session.id, pid);
      }
      refresh();
    } catch (err) {
      onError(`Failed to resume: ${err}`);
    } finally {
      setLaunching(false);
    }
  }, [worktree, refresh, onError]);

  const handleFocus = useCallback(() => {
    try {
      openClaudeInTerminal(worktree.path, { continueSession: true });
    } catch (err) {
      onError(`Failed to focus: ${err}`);
    }
  }, [worktree, onError]);

  const handleKill = useCallback((session: AgentSession) => {
    if (session.pid != null) {
      killClaudeSession(session.pid);
    }
    removeAgentSession(session.id);
    setSelectedIndex((i) => Math.max(0, Math.min(i, sessions.length - 2)));
    refresh();
  }, [refresh, sessions.length]);

  useInput((input, key) => {
    if (subMode !== "list") return;

    if (key.escape) {
      onBack();
      return;
    }

    if (key.upArrow || input === "k") {
      setSelectedIndex((i) => Math.max(0, i - 1));
      return;
    }
    if (key.downArrow || input === "j") {
      setSelectedIndex((i) => Math.min(sessions.length - 1, i + 1));
      return;
    }

    if (input === "n" && !launching) {
      // Check if selected session is resumable
      if (selected && !selected.is_open && selected.session_id) {
        handleResume(selected);
      } else {
        setSubMode("role-select");
      }
      return;
    }

    if (key.return && selected) {
      handleFocus();
      return;
    }

    if (input === "x" && selected) {
      handleKill(selected);
      return;
    }
  }, { isActive: subMode === "list" });

  if (subMode === "role-select") {
    return (
      <RoleSelector
        onSelect={handleLaunch}
        onCancel={() => setSubMode("list")}
      />
    );
  }

  const branchDisplay = worktree.custom_name ?? worktree.branch;

  return (
    <Box flexDirection="column" borderStyle="single" borderColor="cyan" paddingX={1} flexGrow={1}>
      <Text bold color="cyan">Managed: {branchDisplay}</Text>

      {/* Session list */}
      <Box flexDirection="column" marginTop={1}>
        <Text bold>Agents ({sessions.length})</Text>
        {sessions.length === 0 && !launching && (
          <Text dimColor>No agents yet. Press [n] to launch one.</Text>
        )}
        {launching && sessions.length === 0 && (
          <Text dimColor>Launching...</Text>
        )}
        {sessions.map((s, i) => {
          const isSelected = i === selectedIndex;
          const pointer = isSelected ? "\u25B8" : " ";
          const dot = statusDot(s.status, s.is_open);
          const color = statusColor(s.status);
          const roleLabel = s.role_name ?? "(no role)";
          const pidLabel = s.pid ? `PID ${s.pid}` : "";
          const resumable = !s.is_open && s.session_id;
          const lastMsg = s.last_response ?? s.transcript_summary;
          return (
            <Box key={s.id} flexDirection="column">
              <Text>
                <Text color={isSelected ? "cyan" : undefined}>{pointer} </Text>
                <Text color={color}>{dot}</Text>
                <Text> #{i + 1} </Text>
                <Text color={isSelected ? "cyan" : undefined}>{roleLabel}</Text>
                <Text>  </Text>
                <Text>{statusLabel(s.status)}</Text>
                {pidLabel && <Text dimColor>  {pidLabel}</Text>}
                <Text dimColor>  {timeAgo(s.updated_at)}</Text>
                {resumable ? <Text color="yellow"> (resume)</Text> : null}
              </Text>
              {lastMsg && (
                <Text wrap="truncate-end" dimColor>{"    "}{lastMsg.replace(/\n/g, " ")}</Text>
              )}
            </Box>
          );
        })}
      </Box>

      {/* Detail for selected session */}
      {selected && (
        <Box flexDirection="column" marginTop={1}>
          <Text dimColor>{"─".repeat(50)}</Text>
          <Text bold>Detail</Text>
          <Text>Role: {selected.role_name ?? "(none)"}</Text>
          {selected.session_id && <Text dimColor>Session: {selected.session_id}</Text>}
          <Text>Status: <Text color={statusColor(selected.status)}>{statusLabel(selected.status)}</Text></Text>
          {selected.transcript_summary && (
            <Text wrap="truncate-end">Task: {selected.transcript_summary.slice(0, 200)}</Text>
          )}
          {selected.last_response && (
            <Text wrap="truncate-end" dimColor>Last: {selected.last_response.slice(0, 200)}</Text>
          )}
        </Box>
      )}

      {/* Completed work */}
      {completedWork.length > 0 && (
        <Box flexDirection="column" marginTop={1}>
          <Text dimColor>{"─".repeat(50)}</Text>
          <Text bold>Completed Work</Text>
          {completedWork.slice(0, 5).map((cw, i) => (
            <Text key={i} wrap="truncate-end">
              <Text color="blueBright">{cw.roleName ?? "agent"}: </Text>
              <Text>{cw.summary.slice(0, 150)}</Text>
            </Text>
          ))}
        </Box>
      )}

      {/* Action bar */}
      <Box marginTop={1} borderStyle="single" borderTop borderBottom={false} borderLeft={false} borderRight={false} paddingX={0}>
        <Text>
          <Text color="yellow">[Enter]</Text><Text dimColor>focus </Text>
          <Text color="yellow">[n]</Text><Text dimColor>{selected && !selected.is_open && selected.session_id ? "resume" : "ew"} </Text>
          <Text color="yellow">[x]</Text><Text dimColor>kill </Text>
          <Text color="yellow">[Esc]</Text><Text dimColor>back </Text>
          <Text color="yellow">[j/k]</Text><Text dimColor>nav</Text>
        </Text>
      </Box>
    </Box>
  );
}
