import React from "react";
import { Box } from "ink";
import { StatusBar } from "./StatusBar.js";
import { WorktreeList } from "./WorktreeList.js";
import { WorktreeDetail } from "./WorktreeDetail.js";
import { ActionBar } from "./ActionBar.js";
import type { WorktreeWithStatus, WorktreeGroup } from "../lib/types.js";

interface DashboardProps {
  repoName: string;
  groups: WorktreeGroup[];
  flatWorktrees: WorktreeWithStatus[];
  selectedIndex: number;
  busy: string | null;
  escHint: boolean;
  unseenIds: Set<string>;
  compactView: boolean;
}

export function Dashboard({
  repoName,
  groups,
  flatWorktrees,
  selectedIndex,
  busy,
  escHint,
  unseenIds,
  compactView,
}: DashboardProps) {
  const selected = flatWorktrees[selectedIndex] ?? null;

  return (
    <Box flexDirection="column">
      <StatusBar repoName={repoName} worktreeCount={flatWorktrees.length} repoCount={groups.length} />
      <Box>
        <WorktreeList groups={groups} flatWorktrees={flatWorktrees} selectedIndex={selectedIndex} unseenIds={unseenIds} compactView={compactView} />
        <WorktreeDetail worktree={selected} />
      </Box>
      <ActionBar busy={busy} hasWorktrees={flatWorktrees.length > 0} escHint={escHint} />
    </Box>
  );
}
