import React from "react";
import { Box, Text } from "ink";
import type { WorktreeWithStatus } from "../lib/types.js";

interface WorktreeDetailProps {
  worktree: WorktreeWithStatus | null;
}

function statusColor(status: string | undefined): string {
  switch (status) {
    case "executing":
    case "thinking":
    case "planning":
      return "green";
    case "waiting_for_input":
      return "yellow";
    case "error":
      return "red";
    default:
      return "gray";
  }
}

function statusLabel(status: string | undefined): string {
  switch (status) {
    case "executing":
      return "Executing";
    case "thinking":
      return "Thinking";
    case "planning":
      return "Planning";
    case "waiting_for_input":
      return "Waiting for input";
    case "error":
      return "Error";
    case "completed":
      return "Completed";
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

  return (
    <Box
      flexDirection="column"
      borderStyle="single"
      flexGrow={1}
      paddingX={1}
    >
      <Text bold> Detail</Text>

      <Box flexDirection="column" marginTop={1} gap={1}>
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

        {/* Claude Agent Status */}
        <Box flexDirection="column">
          <Text bold>Claude</Text>
          <Text>
            <Text color={statusColor(status)}>●</Text>{" "}
            <Text>{statusLabel(status)}</Text>
            {worktree.agent_status?.plan_mode ? (
              <Text color="magenta"> [Plan Mode]</Text>
            ) : null}
          </Text>
        </Box>

        {/* Last Response */}
        {worktree.agent_status?.last_response && (
          <Box flexDirection="column">
            <Text bold>Last Response</Text>
            <Text wrap="truncate-end">
              {worktree.agent_status.last_response.slice(0, 300)}
            </Text>
          </Box>
        )}
      </Box>
    </Box>
  );
}
