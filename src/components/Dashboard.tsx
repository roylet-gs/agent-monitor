import React from "react";
import { Box } from "ink";
import { StatusBar, type UpdateInfo } from "./StatusBar.js";
import { WorktreeList } from "./WorktreeList.js";
import { WorktreeDetail } from "./WorktreeDetail.js";
import { ActionBar } from "./ActionBar.js";
import { ManagedActionBar } from "./ManagedActionBar.js";
import { LogPanel } from "./LogPanel.js";
import type { WorktreeWithStatus, WorktreeGroup, StandaloneSession, PendingInput } from "../lib/types.js";

interface DashboardProps {
  repoName: string;
  groups: WorktreeGroup[];
  flatWorktrees: WorktreeWithStatus[];
  standaloneSessions: StandaloneSession[];
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
  ideIsTerm?: boolean;
  integrationLoading?: string | null;
  managedMode?: boolean;
  pendingInputs?: Map<string, PendingInput>;
}

export const Dashboard = React.memo(function Dashboard({
  repoName,
  groups,
  flatWorktrees,
  standaloneSessions,
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
  ideIsTerm,
  integrationLoading,
  managedMode,
  pendingInputs,
}: DashboardProps) {
  const isStandaloneSelected = selectedIndex >= flatWorktrees.length;
  const selectedWorktree = isStandaloneSelected ? null : (flatWorktrees[selectedIndex] ?? null);
  const selectedStandalone = isStandaloneSelected
    ? (standaloneSessions[selectedIndex - flatWorktrees.length] ?? null)
    : null;

  // Find pending input for the selected worktree
  const selectedPendingInput = selectedWorktree && pendingInputs
    ? Array.from(pendingInputs.values()).find(pi => pi.worktreeId === selectedWorktree.id) ?? null
    : null;

  const selectedStatus = selectedWorktree?.agent_status?.status;
  const selectedCanMessage = managedMode && !selectedPendingInput &&
    (selectedStatus === "idle" || selectedStatus === "done") &&
    !!selectedWorktree?.agent_status?.session_id;

  return (
    <Box flexDirection="column" flexGrow={1}>
      <StatusBar repoName={repoName} worktreeCount={flatWorktrees.length} repoCount={groups.length} standaloneCount={standaloneSessions.length} version={version} updateInfo={updateInfo} />
      <Box flexGrow={1}>
        <WorktreeList groups={groups} flatWorktrees={flatWorktrees} standaloneSessions={standaloneSessions} standaloneStartIndex={flatWorktrees.length} selectedIndex={selectedIndex} unseenIds={unseenIds} compactView={compactView} />
        <WorktreeDetail worktree={selectedWorktree} standaloneSession={selectedStandalone} pendingInput={selectedPendingInput} managedMode={managedMode} />
      </Box>
      {showLogs && <LogPanel height={Math.max(5, Math.floor(terminalRows / 3))} />}
      {managedMode && (
        <ManagedActionBar
          pendingInput={selectedPendingInput}
          canMessage={!!selectedCanMessage}
          pendingCount={pendingInputs?.size ?? 0}
        />
      )}
      <ActionBar busy={busy} hasWorktrees={flatWorktrees.length > 0 || standaloneSessions.length > 0} escHint={escHint} ghPrStatus={ghPrStatus} linearEnabled={linearEnabled} hasPr={!!selectedWorktree?.pr_info} hasLinear={!!selectedWorktree?.linear_info} ideIsTerm={ideIsTerm} integrationLoading={integrationLoading} />
    </Box>
  );
});
