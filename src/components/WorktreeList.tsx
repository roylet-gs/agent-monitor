import React from "react";
import { Box, Text } from "ink";
import type { WorktreeWithStatus } from "../lib/types.js";

interface WorktreeListProps {
  worktrees: WorktreeWithStatus[];
  selectedIndex: number;
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

function statusLabel(wt: WorktreeWithStatus): string {
  const status = wt.agent_status?.status;
  const planMode = wt.agent_status?.plan_mode;
  const prefix = planMode ? "P:" : "";
  switch (status) {
    case "executing":
      return `${prefix}Exec`;
    case "thinking":
      return `${prefix}Think`;
    case "planning":
      return `${prefix}Plan`;
    case "waiting_for_input":
      return `${prefix}Wait`;
    case "error":
      return `${prefix}Err`;
    case "idle":
      return planMode ? "P:Idle" : "";
    case "completed":
      return "";
    default:
      return "";
  }
}

export function WorktreeList({ worktrees, selectedIndex }: WorktreeListProps) {
  if (worktrees.length === 0) {
    return (
      <Box
        flexDirection="column"
        borderStyle="single"
        width="40%"
        paddingX={1}
      >
        <Text bold> Worktrees</Text>
        <Box marginTop={1}>
          <Text dimColor>No worktrees. Press [n] to create one.</Text>
        </Box>
      </Box>
    );
  }

  return (
    <Box
      flexDirection="column"
      borderStyle="single"
      width="40%"
      paddingX={1}
    >
      <Text bold> Worktrees</Text>
      <Box flexDirection="column" marginTop={1}>
        {worktrees.map((wt, i) => {
          const isSelected = i === selectedIndex;
          const displayName = wt.custom_name ?? wt.branch;
          return (
            <Box key={wt.id} gap={1}>
              <Text>{isSelected ? "▸" : " "}</Text>
              <Text color={statusColor(wt.agent_status?.status)}>●</Text>
              <Text
                bold={isSelected}
                color={isSelected ? "cyan" : undefined}
              >
                {displayName}
              </Text>
              {statusLabel(wt) ? (
                <Text color={statusColor(wt.agent_status?.status)}>
                  {statusLabel(wt)}
                </Text>
              ) : null}
            </Box>
          );
        })}
      </Box>
    </Box>
  );
}
