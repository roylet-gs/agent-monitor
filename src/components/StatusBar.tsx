import React from "react";
import { Box, Text } from "ink";

interface StatusBarProps {
  repoName: string;
  worktreeCount: number;
}

export function StatusBar({ repoName, worktreeCount }: StatusBarProps) {
  return (
    <Box justifyContent="space-between" paddingX={1}>
      <Text bold color="cyan">
        Agent Monitor
      </Text>
      <Text dimColor>
        {repoName} ({worktreeCount} worktree{worktreeCount !== 1 ? "s" : ""})
      </Text>
    </Box>
  );
}
