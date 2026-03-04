import React from "react";
import { Box, Text } from "ink";
import { getPrStatusLabel } from "../lib/github.js";
import { getLinearStatusColor } from "../lib/linear.js";
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

        {/* Pull Request */}
        {worktree.pr_info && (() => {
          const { label, color } = getPrStatusLabel(worktree.pr_info);
          const pr = worktree.pr_info;
          const checks = pr.checksStatus !== "none" ? (() => {
            const icon = pr.checksStatus === "passing" ? "✓" : pr.checksStatus === "failing" ? "✗" : "◌";
            const checkColor = pr.checksStatus === "passing" ? "green" : pr.checksStatus === "failing" ? "red" : "cyan";
            return { icon, checkColor, statusText: pr.checksStatus === "pending" ? "running" : pr.checksStatus };
          })() : null;
          return (
            <Box flexDirection="column">
              <Text>
                <Text bold>PR #{pr.number} </Text>
                <Text color={color}>{label}</Text>
              </Text>
              <Text dimColor>{pr.title}</Text>
              {checks && (
                <Text color={checks.checkColor}>
                  {checks.icon} Checks {checks.statusText}
                </Text>
              )}
            </Box>
          );
        })()}

        {/* Linear Ticket */}
        {worktree.linear_info && (
          <Box flexDirection="column">
            <Text>
              <Text bold>Linear </Text>
              <Text bold color={getLinearStatusColor(worktree.linear_info.state.type)}>
                {worktree.linear_info.identifier}
              </Text>
              <Text> </Text>
              <Text color={getLinearStatusColor(worktree.linear_info.state.type)}>
                {worktree.linear_info.state.name}
              </Text>
            </Text>
            <Text dimColor>{worktree.linear_info.title}</Text>
            <Text dimColor>
              Priority: {worktree.linear_info.priorityLabel}
              {worktree.linear_info.assignee && ` · Assigned: ${worktree.linear_info.assignee}`}
            </Text>
            <Text dimColor>{worktree.linear_info.url}</Text>
          </Box>
        )}

        {/* Git Info */}
        <Box flexDirection="column">
          <Text bold>Git</Text>
          <Text dimColor>{worktree.branch}</Text>
          {worktree.last_commit && (
            <Text dimColor>{worktree.last_commit.message} ({worktree.last_commit.relative_time})</Text>
          )}
          {worktree.git_status && (
            <Text>
              <Text color="green">↑{worktree.git_status.ahead}</Text>{" "}
              <Text color="red">↓{worktree.git_status.behind}</Text>
              {worktree.git_status.dirty > 0 && (
                <Text color="yellow">
                  {"  "}✎ {worktree.git_status.dirty} dirty
                </Text>
              )}
            </Text>
          )}
        </Box>
      </Box>
    </Box>
  );
}
