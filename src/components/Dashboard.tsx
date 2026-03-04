import React from "react";
import { Box } from "ink";
import { StatusBar } from "./StatusBar.js";
import { WorktreeList } from "./WorktreeList.js";
import { WorktreeDetail } from "./WorktreeDetail.js";
import { ActionBar } from "./ActionBar.js";
import type { WorktreeWithStatus } from "../lib/types.js";

interface DashboardProps {
  repoName: string;
  worktrees: WorktreeWithStatus[];
  selectedIndex: number;
  busy: string | null;
  escHint: boolean;
  unseenIds: Set<string>;
}

export function Dashboard({
  repoName,
  worktrees,
  selectedIndex,
  busy,
  escHint,
  unseenIds,
}: DashboardProps) {
  const selected = worktrees[selectedIndex] ?? null;

  return (
    <Box flexDirection="column">
      <StatusBar repoName={repoName} worktreeCount={worktrees.length} />
      <Box>
        <WorktreeList worktrees={worktrees} selectedIndex={selectedIndex} unseenIds={unseenIds} />
        <WorktreeDetail worktree={selected} />
      </Box>
      <ActionBar busy={busy} hasWorktrees={worktrees.length > 0} escHint={escHint} />
    </Box>
  );
}
