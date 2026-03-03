import React from "react";
import { Box, Text, useInput } from "ink";
import type { WorktreeWithStatus } from "../lib/types.js";

interface DeleteConfirmProps {
  worktree: WorktreeWithStatus;
  onConfirm: () => void;
  onCancel: () => void;
}

export function DeleteConfirm({
  worktree,
  onConfirm,
  onCancel,
}: DeleteConfirmProps) {
  useInput((input, key) => {
    if (key.escape || input === "n") {
      onCancel();
      return;
    }
    if (key.return || input === "y") {
      onConfirm();
    }
  });

  const dirty = worktree.git_status?.dirty ?? 0;
  const ahead = worktree.git_status?.ahead ?? 0;
  const hasWarnings = dirty > 0 || ahead > 0;

  return (
    <Box
      flexDirection="column"
      borderStyle="single"
      paddingX={1}
      borderColor={hasWarnings ? "yellow" : undefined}
    >
      <Text bold color="yellow">
        ⚠ Delete worktree "{worktree.custom_name ?? worktree.branch}"?
      </Text>

      {dirty > 0 && (
        <Text color="yellow">
          {"  "}⚠ {dirty} uncommitted change{dirty !== 1 ? "s" : ""}
        </Text>
      )}

      {ahead > 0 && (
        <Text color="yellow">
          {"  "}⚠ {ahead} commit{ahead !== 1 ? "s" : ""} not pushed to remote
        </Text>
      )}

      <Box marginTop={1}>
        <Text>
          <Text color="yellow">[Enter/y]</Text> Confirm{" "}
          <Text color="yellow">[Esc/n]</Text> Cancel
        </Text>
      </Box>
    </Box>
  );
}
