import React from "react";
import { Box, Text } from "ink";
import { getPrStatusLabel } from "../lib/github.js";
import { getLinearStatusColor } from "../lib/linear.js";
import type { WorktreeWithStatus } from "../lib/types.js";

interface WorktreeListProps {
  worktrees: WorktreeWithStatus[];
  selectedIndex: number;
  unseenIds: Set<string>;
  compactView: boolean;
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

export function WorktreeList({ worktrees, selectedIndex, unseenIds, compactView }: WorktreeListProps) {
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

          // Build inline metadata (shown on line 1 when no custom name)
          const inlineMeta: React.ReactNode[] = [];

          if (wt.linear_info) {
            inlineMeta.push(
              <Text key="linear" color={getLinearStatusColor(wt.linear_info.state.type)}>{wt.linear_info.identifier}</Text>
            );
          }

          if (wt.pr_info) {
            const { label, color } = getPrStatusLabel(wt.pr_info);
            inlineMeta.push(
              <Text key="pr" color={color} dimColor>{label}</Text>
            );
          }


          return (
            <Box key={wt.id} flexDirection="column" marginBottom={!compactView && wt.custom_name && i < worktrees.length - 1 ? 1 : 0}>
              {/* Line 1: selector + status dot + name + inline meta (if no custom name) */}
              <Box gap={1}>
                <Text>{isSelected ? "▸" : " "}</Text>
                <Text color={statusColor(wt.agent_status?.status)}>●</Text>
                <Text
                  bold={isSelected}
                  color={isSelected ? "cyan" : undefined}
                >
                  {displayName}
                </Text>
                {!wt.custom_name && inlineMeta}
                {unseen && <Text color="magenta" bold>*</Text>}
              </Box>
              {/* Line 2: branch + metadata (only when custom name is set) */}
              {wt.custom_name && (
                <Box paddingLeft={5} gap={1}>
                  <Text dimColor>{wt.branch}</Text>
                  {inlineMeta}
                </Box>
              )}
            </Box>
          );
        })}
      </Box>
    </Box>
  );
}
