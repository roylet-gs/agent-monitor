import React, { useState } from "react";
import { Box, Text, useInput } from "ink";
import type { DiscoveredSession } from "../lib/claude-session.js";

interface SessionPickerProps {
  worktreeName: string;
  worktreePath: string;
  sessions: DiscoveredSession[];
  /** Id of the worktree's current managed session, if any. */
  activeSessionId: string | null;
  onSelect: (session: DiscoveredSession) => void;
  onCancel: () => void;
}

function relativeCwd(cwd: string, worktreePath: string): string {
  if (cwd === worktreePath) return ".";
  return cwd.startsWith(worktreePath + "/") ? cwd.slice(worktreePath.length + 1) : cwd;
}

function age(mtimeMs: number): string {
  const seconds = Math.max(0, Math.round((Date.now() - mtimeMs) / 1000));
  if (seconds < 60) return `${seconds}s ago`;
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 48) return `${hours}h ago`;
  return `${Math.round(hours / 24)}d ago`;
}

const PROMPT_MAX = 60;

export function SessionPicker({ worktreeName, worktreePath, sessions, activeSessionId, onSelect, onCancel }: SessionPickerProps) {
  const [selectedIndex, setSelectedIndex] = useState(() => {
    const active = sessions.findIndex((s) => s.id === activeSessionId);
    return active >= 0 ? active : 0;
  });

  useInput((input, key) => {
    if (key.escape) {
      onCancel();
      return;
    }
    if (input === "j" || key.downArrow) {
      setSelectedIndex((i) => Math.min(i + 1, sessions.length - 1));
      return;
    }
    if (input === "k" || key.upArrow) {
      setSelectedIndex((i) => Math.max(i - 1, 0));
      return;
    }
    if (key.return) {
      const session = sessions[selectedIndex];
      if (session) onSelect(session);
      return;
    }
  });

  return (
    <Box flexDirection="column" borderStyle="single" paddingX={1}>
      <Text bold color="cyan">
        Claude Sessions — {worktreeName}
      </Text>
      <Box marginTop={1} flexDirection="column">
        {sessions.map((session, i) => {
          const selected = i === selectedIndex;
          const prompt = session.lastPrompt
            ? session.lastPrompt.length > PROMPT_MAX
              ? session.lastPrompt.slice(0, PROMPT_MAX - 1) + "…"
              : session.lastPrompt
            : "(no messages)";
          return (
            <Box key={session.id} flexDirection="column">
              <Box gap={1}>
                <Text>{selected ? "▸" : " "}</Text>
                <Text bold={selected} color={selected ? "cyan" : undefined}>
                  {session.id.slice(0, 8)}
                </Text>
                <Text dimColor>{relativeCwd(session.cwd, worktreePath)}</Text>
                <Text dimColor>{age(session.mtimeMs)}</Text>
                {session.id === activeSessionId && <Text color="green">(active)</Text>}
              </Box>
              <Box marginLeft={2}>
                <Text dimColor wrap="truncate-end">❯ {prompt}</Text>
              </Box>
            </Box>
          );
        })}
      </Box>
      <Box
        marginTop={1}
        borderStyle="single"
        borderTop
        borderBottom={false}
        borderLeft={false}
        borderRight={false}
      >
        <Text>
          <Text color="yellow">[↑↓]</Text> Navigate{" "}
          <Text color="yellow">[Enter]</Text> Chat{" "}
          <Text color="yellow">[Esc]</Text> Cancel
        </Text>
      </Box>
    </Box>
  );
}
