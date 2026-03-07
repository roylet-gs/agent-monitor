import React from "react";
import { Box, Text } from "ink";
import { getPrStatusLabel } from "../lib/github.js";
import { getLinearStatusColor } from "../lib/linear.js";
import { isEffectivelyOpen } from "../lib/agent-utils.js";
import type { WorktreeWithStatus, WorktreeGroup } from "../lib/types.js";

interface WorktreeListProps {
  groups: WorktreeGroup[];
  flatWorktrees: WorktreeWithStatus[];
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

export const WorktreeList = React.memo(function WorktreeList({ groups, flatWorktrees, selectedIndex, unseenIds, compactView }: WorktreeListProps) {
  if (flatWorktrees.length === 0) {
    return (
      <Box
        flexDirection="column"
        borderStyle="single"
        width="40%"
        minWidth="40%"
        paddingX={1}
      >
        <Text bold> Worktrees</Text>
        <Box marginTop={1}>
          <Text dimColor>No worktrees. Press [n] to create one.</Text>
        </Box>
      </Box>
    );
  }

  const showHeaders = groups.length > 1;
  let flatIdx = 0;

  return (
    <Box
      flexDirection="column"
      borderStyle="single"
      width="40%"
      minWidth="40%"
      paddingX={1}
    >
      <Text bold> Worktrees</Text>
      <Box flexDirection="column" marginTop={1}>
        {groups.map((group) => {
          const groupWorktrees = group.worktrees;
          const startIdx = flatIdx;

          const renderedItems = groupWorktrees.map((wt, i) => {
            const currentFlatIdx = startIdx + i;
            const isSelected = currentFlatIdx === selectedIndex;
            const displayName = wt.custom_name ?? wt.branch;
            const unseen = unseenIds.has(wt.id);
            const open = isEffectivelyOpen(wt.agent_status);

            const isBranchOnly = wt.is_main === 1 && wt.branch !== "main" && wt.branch !== "master";
            const inlineMeta: React.ReactNode[] = [];

            if (isBranchOnly) {
              inlineMeta.push(
                <Text key="branch-only" dimColor>[branch]</Text>
              );
            }

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
              <Box key={wt.id} flexDirection="column" marginBottom={!compactView && wt.custom_name && i < groupWorktrees.length - 1 ? 1 : 0}>
                <Box gap={1}>
                  <Text>{isSelected ? "▸" : " "}</Text>
                  {open ? <Text color={statusColor(wt.agent_status?.status)}>●</Text> : <Text dimColor>○</Text>}
                  <Text
                    bold={isSelected}
                    color={isSelected ? "cyan" : undefined}
                  >
                    {displayName}
                  </Text>
                  {!wt.custom_name && inlineMeta}
                  {unseen && <Text color="magenta" bold>*</Text>}
                </Box>
                {wt.custom_name && (
                  <Box paddingLeft={5} gap={1}>
                    <Text dimColor>{wt.branch}</Text>
                    {inlineMeta}
                  </Box>
                )}
              </Box>
            );
          });

          flatIdx += groupWorktrees.length;

          return (
            <Box key={group.repo.id} flexDirection="column" marginTop={showHeaders && startIdx > 0 ? 1 : 0}>
              {showHeaders && (
                <Text dimColor>── {group.repo.name} ───</Text>
              )}
              {renderedItems}
            </Box>
          );
        })}
      </Box>
    </Box>
  );
});
