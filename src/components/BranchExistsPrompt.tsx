import React, { useState, useEffect } from "react";
import { Box, Text, useInput } from "ink";

interface BranchExistsPromptProps {
  branchName: string;
  onReuse: () => void;
  onDeleteAndRecreate: (deleteRemote: boolean) => void;
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
  const [phase, setPhase] = useState<"choose" | "confirm-remote">("choose");

  useEffect(() => {
    const timer = setTimeout(() => setReady(true), 50);
    return () => clearTimeout(timer);
  }, []);

  useInput((input, key) => {
    if (!ready) return;

    if (phase === "choose") {
      if (key.escape || input === "n") {
        onCancel();
        return;
      }
      if (key.return || input === "y") {
        onReuse();
      }
      if (input === "d") {
        setPhase("confirm-remote");
        setReady(false);
        setTimeout(() => setReady(true), 50);
      }
    } else if (phase === "confirm-remote") {
      if (key.escape) {
        setPhase("choose");
        setReady(false);
        setTimeout(() => setReady(true), 50);
        return;
      }
      if (input === "y") {
        onDeleteAndRecreate(true);
      }
      if (key.return || input === "n") {
        onDeleteAndRecreate(false);
      }
    }
  });

  if (phase === "confirm-remote") {
    return (
      <Box flexDirection="column" borderStyle="single" paddingX={1} borderColor="yellow">
        <Text bold color="yellow">
          Also delete remote branch?
        </Text>
        <Box marginTop={1} flexDirection="column">
          <Text>
            Branch "<Text color="cyan">{branchName}</Text>" may also exist on the remote (origin).
          </Text>
        </Box>
        <Box marginTop={1} flexDirection="column">
          <Text>
            <Text color="red">[y]</Text> Yes, delete remote branch too
          </Text>
          <Text>
            <Text color="yellow">[Enter/n]</Text> No, only delete local
          </Text>
          <Text>
            <Text color="yellow">[Esc]</Text> Go back
          </Text>
        </Box>
      </Box>
    );
  }

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
          <Text color="yellow">[Enter/y]</Text> Reuse existing branch (pulls latest from remote)
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
