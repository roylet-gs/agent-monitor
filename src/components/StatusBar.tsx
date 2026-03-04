import React from "react";
import { Box, Text } from "ink";

export interface UpdateInfo {
  current: string;
  latest: string;
  updateAvailable: boolean;
}

interface StatusBarProps {
  repoName: string;
  worktreeCount: number;
  repoCount?: number;
  version?: string;
  updateInfo?: UpdateInfo | null;
}

export function StatusBar({ repoName, worktreeCount, repoCount, version, updateInfo }: StatusBarProps) {
  const repoLabel = repoCount && repoCount > 1
    ? `${repoCount} repos`
    : repoName;

  return (
    <Box justifyContent="space-between" paddingX={1}>
      <Box>
        <Text bold color="cyan">
          Agent Monitor
        </Text>
        {version && <Text dimColor> v{version}</Text>}
        {updateInfo?.updateAvailable && (
          <Text color="yellow">
            {" "}v{updateInfo.current} → v{updateInfo.latest} available! [u]pdate
          </Text>
        )}
      </Box>
      <Text dimColor>
        {repoLabel} ({worktreeCount} worktree{worktreeCount !== 1 ? "s" : ""})
      </Text>
    </Box>
  );
}
