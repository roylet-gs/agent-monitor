import React from "react";
import { Box, Text } from "ink";
import type { WorktreeWithStatus } from "../lib/types.js";

interface WorktreeListProps {
  worktrees: WorktreeWithStatus[];
  selectedIndex: number;
  unseenIds: Set<string>;
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

export function WorktreeList({ worktrees, selectedIndex, unseenIds }: WorktreeListProps) {
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
          const unseen = unseenIds.has(wt.id);
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
              {unseen && <Text color="magenta" bold>*</Text>}
            </Box>
          );
        })}
      </Box>
    </Box>
  );
}
