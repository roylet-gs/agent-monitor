import React from "react";
import { Box, Text } from "ink";

interface StatusBarProps {
  repoName: string;
  worktreeCount: number;
  repoCount?: number;
  version?: string;
}

export function StatusBar({ repoName, worktreeCount, repoCount, version }: StatusBarProps) {
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
      </Box>
      <Text dimColor>
        {repoLabel} ({worktreeCount} worktree{worktreeCount !== 1 ? "s" : ""})
      </Text>
    </Box>
  );
}
