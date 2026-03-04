import React from "react";
import { Box, Text } from "ink";
import type { WorktreeWithStatus } from "../lib/types.js";

interface WorktreeDetailProps {
  worktree: WorktreeWithStatus | null;
}

function statusColor(status: string | undefined): string {
  switch (status) {
    case "executing":
      return "green";
    case "planning":
      return "cyan";
    case "waiting":
      return "yellow";
    default:
      return "gray";
  }
}

function statusLabel(status: string | undefined): string {
  switch (status) {
    case "executing":
      return "Executing";
    case "planning":
      return "Planning";
    case "waiting":
      return "Waiting";
    case "idle":
      return "Idle";
    default:
      return "Unknown";
  }
}

export function WorktreeDetail({ worktree }: WorktreeDetailProps) {
  if (!worktree) {
    return (
      <Box
        flexDirection="column"
        borderStyle="single"
        flexGrow={1}
        paddingX={1}
      >
        <Text bold> Detail</Text>
        <Box marginTop={1}>
          <Text dimColor>Select a worktree to view details</Text>
        </Box>
      </Box>
    );
  }

  const status = worktree.agent_status?.status;
  const isActive = status === "executing" || status === "planning";

  // Contextual response: show transcript_summary as "Task" when active, last_response as "Last Response" when idle
  const responseText = isActive
    ? (worktree.agent_status?.transcript_summary ?? worktree.agent_status?.last_response)
    : (worktree.agent_status?.last_response ?? worktree.agent_status?.transcript_summary);
  const responseLabel = isActive ? "Task" : "Last Response";

  return (
    <Box
      flexDirection="column"
      borderStyle="single"
      flexGrow={1}
      paddingX={1}
    >
      <Text bold> Detail</Text>

      <Box flexDirection="column" marginTop={1} gap={1}>
        {/* Claude Agent Status — first section */}
        <Box>
          <Text bold>Claude </Text>
          <Text color={statusColor(status)}>● </Text>
          <Text>{statusLabel(status)}</Text>
        </Box>

        {/* Contextual Response */}
        {responseText && (
          <Box flexDirection="column">
            <Text bold>{responseLabel}</Text>
            <Text wrap="truncate-end">
              {responseText.slice(0, 300)}
            </Text>
          </Box>
        )}

        {/* Name / Branch */}
        <Box flexDirection="column">
          {worktree.custom_name && (
            <Text>
              <Text bold>Name </Text>
              <Text>{worktree.custom_name}</Text>
            </Text>
          )}
          <Text>
            <Text bold>Branch </Text>
            <Text>{worktree.branch}</Text>
          </Text>
        </Box>

        {/* Last Commit */}
        {worktree.last_commit && (
          <Box flexDirection="column">
            <Text bold>Last Commit</Text>
            <Text>{worktree.last_commit.message}</Text>
            <Text dimColor>{worktree.last_commit.relative_time}</Text>
          </Box>
        )}

        {/* Git Status */}
        {worktree.git_status && (
          <Box flexDirection="column">
            <Text bold>Git Status</Text>
            <Text>
              <Text color="green">↑{worktree.git_status.ahead}</Text>{" "}
              <Text color="red">↓{worktree.git_status.behind}</Text>{" "}
              ahead/behind{"  "}
              {worktree.git_status.dirty > 0 && (
                <Text color="yellow">
                  ✎ {worktree.git_status.dirty} file
                  {worktree.git_status.dirty !== 1 ? "s" : ""} dirty
                </Text>
              )}
            </Text>
          </Box>
        )}
      </Box>
    </Box>
  );
}
