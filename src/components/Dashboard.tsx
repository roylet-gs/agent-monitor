import React from "react";
import { Box } from "ink";
import { StatusBar, type UpdateInfo } from "./StatusBar.js";
import { WorktreeList } from "./WorktreeList.js";
import { WorktreeDetail } from "./WorktreeDetail.js";
import { ActionBar } from "./ActionBar.js";
import { LogPanel } from "./LogPanel.js";
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
  showLogs: boolean;
  terminalRows: number;
  version?: string;
  updateInfo?: UpdateInfo | null;
  ghPrStatus?: boolean;
  linearEnabled?: boolean;
  terminalOpenIds: Set<string>;
  ide: string;
  modifierHeld: boolean;
}

export const Dashboard = React.memo(function Dashboard({
  repoName,
  groups,
  flatWorktrees,
  selectedIndex,
  busy,
  escHint,
  unseenIds,
  compactView,
  showLogs,
  terminalRows,
  version,
  updateInfo,
  ghPrStatus,
  linearEnabled,
  terminalOpenIds,
  ide,
  modifierHeld,
}: DashboardProps) {
  const selected = flatWorktrees[selectedIndex] ?? null;

  return (
    <Box flexDirection="column" flexGrow={1}>
      <StatusBar repoName={repoName} worktreeCount={flatWorktrees.length} repoCount={groups.length} version={version} updateInfo={updateInfo} />
      <Box flexGrow={1}>
        <WorktreeList groups={groups} flatWorktrees={flatWorktrees} selectedIndex={selectedIndex} unseenIds={unseenIds} compactView={compactView} terminalOpenIds={terminalOpenIds} ide={ide} />
        <WorktreeDetail worktree={selected} />
      </Box>
      {showLogs && <LogPanel height={Math.max(5, Math.floor(terminalRows / 3))} />}
      <ActionBar busy={busy} hasWorktrees={flatWorktrees.length > 0} escHint={escHint} ghPrStatus={ghPrStatus} linearEnabled={linearEnabled} hasPr={!!selected?.pr_info} hasLinear={!!selected?.linear_info} modifierHeld={modifierHeld} />
    </Box>
  );
});
