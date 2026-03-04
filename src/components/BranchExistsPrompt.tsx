import React, { useState, useEffect } from "react";
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
  // Ignore input on the first frame to avoid the Enter keypress
  // that submitted the previous form from bleeding through
  const [ready, setReady] = useState(false);
  useEffect(() => {
    const timer = setTimeout(() => setReady(true), 50);
    return () => clearTimeout(timer);
  }, []);

  useInput((input, key) => {
    if (!ready) return;
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
