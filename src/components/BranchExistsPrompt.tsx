import React from "react";
import { Box, Text, useInput } from "ink";

interface BranchExistsPromptProps {
  branchName: string;
  onReuse: () => void;
  onCancel: () => void;
}

export function BranchExistsPrompt({
  branchName,
  onReuse,
  onCancel,
}: BranchExistsPromptProps) {
  useInput((input, key) => {
    if (key.escape || input === "n") {
      onCancel();
      return;
    }
    if (key.return || input === "y") {
      onReuse();
    }
  });

  return (
    <Box flexDirection="column" borderStyle="single" paddingX={1} borderColor="yellow">
      <Text bold color="yellow">
        Branch "{branchName}" already exists
      </Text>
      <Box marginTop={1} flexDirection="column">
        <Text>A branch with this name already exists. Would you like to</Text>
        <Text>create a worktree using the existing branch?</Text>
      </Box>
      <Box marginTop={1}>
        <Text>
          <Text color="yellow">[Enter/y]</Text> Reuse branch{" "}
          <Text color="yellow">[Esc/n]</Text> Go back
        </Text>
      </Box>
    </Box>
  );
}
