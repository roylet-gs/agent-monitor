import React, { useState, useEffect } from "react";
import { Box, Text, useInput } from "ink";

interface BranchExistsPromptProps {
  branchName: string;
  localExists: boolean;
  remoteExists: boolean;
  onReuseLocal: () => void;
  onPullRemote: () => void;
  onCreateDisconnected: () => void;
  onDeleteAndRecreate: () => void;
  onCancel: () => void;
}

export function BranchExistsPrompt({
  branchName,
  localExists,
  remoteExists,
  onReuseLocal,
  onPullRemote,
  onCreateDisconnected,
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

    if (remoteExists) {
      // Pull remote is the default action when the branch is on origin.
      if (key.return || input === "p") {
        onPullRemote();
        return;
      }
      if (input === "c") {
        onCreateDisconnected();
        return;
      }
    } else {
      // Local-only: default action is reuse.
      if (key.return || input === "r") {
        onReuseLocal();
        return;
      }
      if (input === "d") {
        onDeleteAndRecreate();
        return;
      }
    }
  });

  const header =
    localExists && remoteExists
      ? `Branch "${branchName}" exists locally and on origin`
      : remoteExists
        ? `Branch "${branchName}" exists on origin`
        : `Branch "${branchName}" already exists locally`;

  return (
    <Box flexDirection="column" borderStyle="single" paddingX={1} borderColor="yellow">
      <Text bold color="yellow">
        {header}
      </Text>
      <Box marginTop={1} flexDirection="column">
        <Text>What would you like to do?</Text>
      </Box>
      <Box marginTop={1} flexDirection="column">
        {remoteExists ? (
          <>
            <Text>
              <Text color="yellow">[Enter/p]</Text>{" "}
              {localExists
                ? "Pull remote (reset local to origin)"
                : "Pull remote (track origin/" + branchName + ")"}
            </Text>
            <Text>
              <Text color={localExists ? "red" : "yellow"}>[c]</Text>{" "}
              {localExists
                ? "Create disconnected (deletes local, no tracking)"
                : "Create disconnected (new local branch from base, no tracking)"}
            </Text>
          </>
        ) : (
          <>
            <Text>
              <Text color="yellow">[Enter/r]</Text> Reuse existing local branch
            </Text>
            <Text>
              <Text color="red">[d]</Text> Delete local branch and re-create from base
            </Text>
          </>
        )}
        <Text>
          <Text color="yellow">[Esc/n]</Text> Go back
        </Text>
      </Box>
    </Box>
  );
}
