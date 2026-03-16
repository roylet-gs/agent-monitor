import React from "react";
import { Box, Text } from "ink";
import { getPrStatusLabel } from "../lib/github.js";
import { getLinearStatusColor } from "../lib/linear.js";
import { isEffectivelyOpen, getDisplayStatus, getDisplayStatusStandalone } from "../lib/agent-utils.js";
import { PulsingDot } from "./PulsingDot.js";
import type { WorktreeWithStatus, WorktreeGroup, StandaloneSession } from "../lib/types.js";
import { homedir } from "os";

interface WorktreeListProps {
  groups: WorktreeGroup[];
  flatWorktrees: WorktreeWithStatus[];
  standaloneSessions: StandaloneSession[];
  standaloneStartIndex: number;
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
    case "done":
      return "blueBright";
    default:
      return "gray";
  }
}

function abbreviatePath(fullPath: string): string {
  const home = homedir();
  let p = fullPath;
  if (p.startsWith(home)) {
    p = "~" + p.slice(home.length);
  }
  const segments = p.split("/").filter(Boolean);
  if (segments.length <= 3) return p;
  return segments.slice(-3).join("/");
}

// Build a map of Linear identifiers that have 2+ worktrees (across all groups)
function buildLinearGroups(flatWorktrees: WorktreeWithStatus[]): Map<string, { title: string; count: number; firstIdx: number }> {
  const counts = new Map<string, { title: string; count: number; firstIdx: number }>();
  for (let i = 0; i < flatWorktrees.length; i++) {
    const id = flatWorktrees[i].linear_info?.identifier;
    if (!id) continue;
    const existing = counts.get(id);
    if (existing) {
      existing.count++;
    } else {
      counts.set(id, { title: flatWorktrees[i].linear_info!.title, count: 1, firstIdx: i });
    }
  }
  // Only keep groups with 2+ worktrees
  for (const [id, info] of counts) {
    if (info.count < 2) counts.delete(id);
  }
  return counts;
}

export const WorktreeList = React.memo(function WorktreeList({ groups, flatWorktrees, standaloneSessions, standaloneStartIndex, selectedIndex, unseenIds, compactView }: WorktreeListProps) {
  const linearGroups = React.useMemo(() => buildLinearGroups(flatWorktrees), [flatWorktrees]);

  if (flatWorktrees.length === 0 && standaloneSessions.length === 0) {
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
      flexShrink={0}
      paddingX={1}
    >
      <Text bold> Worktrees</Text>
      <Box flexDirection="column" marginTop={1}>
        {groups.map((group) => {
          const groupWorktrees = group.worktrees;
          const startIdx = flatIdx;

          // Track which Linear group headers have been emitted within this repo group
          const emittedLinearHeaders = new Set<string>();

          const renderedItems = groupWorktrees.map((wt, i) => {
            const currentFlatIdx = startIdx + i;
            const isSelected = currentFlatIdx === selectedIndex;
            const unseen = unseenIds.has(wt.id);
            const open = isEffectivelyOpen(wt.agent_status);
            const displayStatus = getDisplayStatus(wt.agent_status);

            const linearId = wt.linear_info?.identifier;
            const isInLinearGroup = linearId ? linearGroups.has(linearId) : false;

            // Grouped worktrees show branch name (title is in the header)
            const displayName = isInLinearGroup ? wt.branch : (wt.custom_name ?? wt.branch);
            const showSubline = !isInLinearGroup && !!wt.custom_name;

            // Emit a Linear group header before the first worktree in a multi-worktree group
            let linearHeader: React.ReactNode = null;
            if (isInLinearGroup && linearId && !emittedLinearHeaders.has(linearId)) {
              emittedLinearHeaders.add(linearId);
              const groupInfo = linearGroups.get(linearId)!;
              linearHeader = (
                <Box key={`linear-header-${linearId}`} marginTop={i > 0 ? 1 : 0}>
                  <Text dimColor>── </Text>
                  <Text color={getLinearStatusColor(wt.linear_info!.state.type)}>{linearId}</Text>
                  <Text dimColor>: {groupInfo.title} ──</Text>
                </Box>
              );
            }

            const isBranchOnly = wt.is_main === 1;
            const inlineMeta: React.ReactNode[] = [];

            if (isBranchOnly) {
              inlineMeta.push(
                <Text key="branch-only" dimColor>[branch]</Text>
              );
            }

            // Hide inline Linear badge when it's already shown in the group header
            if (wt.linear_info && !isInLinearGroup) {
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

            const indent = isInLinearGroup ? 1 : 0;

            return (
              <React.Fragment key={wt.id}>
                {linearHeader}
                <Box flexDirection="column" marginBottom={!compactView && showSubline && i < groupWorktrees.length - 1 ? 1 : 0} paddingLeft={indent}>
                  <Box gap={1}>
                    <Text>{isSelected ? "▸" : " "}</Text>
                    {open ? (displayStatus === "executing" || displayStatus === "planning" ? <PulsingDot color={statusColor(displayStatus)} /> : displayStatus === "done" ? <Text color={statusColor("done")}>✓</Text> : <Text color={statusColor(displayStatus)}>●</Text>) : (wt.has_terminal || wt.open_ide) ? <Text color="white">○</Text> : <Text dimColor>○</Text>}
                    <Text
                      bold={isSelected}
                      color={isSelected ? "cyan" : undefined}
                    >
                      {displayName}
                    </Text>
                    {!showSubline && inlineMeta}
                    {unseen && <Text color="magenta" bold>*</Text>}
                  </Box>
                  {showSubline && (
                    <Box paddingLeft={5} gap={1}>
                      <Text dimColor>{wt.branch}</Text>
                      {inlineMeta}
                    </Box>
                  )}
                </Box>
              </React.Fragment>
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
        {standaloneSessions.length > 0 && (
          <Box flexDirection="column" marginTop={flatWorktrees.length > 0 ? 1 : 0}>
            <Text dimColor>── Other Sessions ───</Text>
            {standaloneSessions.map((session, i) => {
              const idx = standaloneStartIndex + i;
              const isSelected = idx === selectedIndex;
              const open = !!session.is_open;
              const unseen = unseenIds.has(session.id);
              const displayStatus = getDisplayStatusStandalone(session);

              return (
                <Box key={session.id} gap={1}>
                  <Text>{isSelected ? "▸" : " "}</Text>
                  {open ? (displayStatus === "executing" || displayStatus === "planning" ? <PulsingDot color={statusColor(displayStatus)} /> : displayStatus === "done" ? <Text color={statusColor("done")}>✓</Text> : <Text color={statusColor(displayStatus)}>●</Text>) : <Text dimColor>○</Text>}
                  <Text
                    bold={isSelected}
                    color={isSelected ? "cyan" : undefined}
                  >
                    {abbreviatePath(session.path)}
                  </Text>
                  {unseen && <Text color="magenta" bold>*</Text>}
                </Box>
              );
            })}
          </Box>
        )}
      </Box>
    </Box>
  );
});
