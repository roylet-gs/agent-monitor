import React, { useState, useEffect } from "react";
import { Box, Text, useInput } from "ink";

interface BranchExistsPromptProps {
  branchName: string;
  onReuse: () => void;
  onDeleteAndRecreate: () => void;
  onCancel: () => void;
}

export function BranchExistsPrompt({
  branchName,
  onReuse,
  onDeleteAndRecreate,
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
    if (input === "d") {
      onDeleteAndRecreate();
    }
  });

  return (
    <Box flexDirection="column" borderStyle="single" paddingX={1} borderColor="yellow">
      <Text bold color="yellow">
        Branch "{branchName}" already exists
      </Text>
      <Box marginTop={1} flexDirection="column">
        <Text>A branch with this name already exists. What would you like to do?</Text>
      </Box>
      <Box marginTop={1} flexDirection="column">
        <Text>
          <Text color="yellow">[Enter/y]</Text> Reuse existing branch
        </Text>
        <Text>
          <Text color="red">[d]</Text> Delete branch and re-create from main
        </Text>
        <Text>
          <Text color="yellow">[Esc/n]</Text> Go back
        </Text>
      </Box>
    </Box>
  );
}
