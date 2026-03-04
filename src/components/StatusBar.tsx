import React from "react";
import { Box, Text } from "ink";

interface StatusBarProps {
  repoName: string;
  worktreeCount: number;
  repoCount?: number;
}

export function StatusBar({ repoName, worktreeCount, repoCount }: StatusBarProps) {
  const repoLabel = repoCount && repoCount > 1
    ? `${repoCount} repos`
    : repoName;

  return (
    <Box justifyContent="space-between" paddingX={1}>
      <Text bold color="cyan">
        Agent Monitor
      </Text>
      <Text dimColor>
        {repoLabel} ({worktreeCount} worktree{worktreeCount !== 1 ? "s" : ""})
      </Text>
    </Box>
  );
}
