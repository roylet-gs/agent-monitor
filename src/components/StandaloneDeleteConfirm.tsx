import React from "react";
import { Box, Text, useInput } from "ink";
import type { StandaloneSession } from "../lib/types.js";

interface StandaloneDeleteConfirmProps {
  session: StandaloneSession;
  onConfirm: () => void;
  onCancel: () => void;
}

export function StandaloneDeleteConfirm({
  session,
  onConfirm,
  onCancel,
}: StandaloneDeleteConfirmProps) {
  useInput((input, key) => {
    if (key.escape || input === "n") {
      onCancel();
      return;
    }
    if (key.return || input === "y") {
      onConfirm();
      return;
    }
  });

  const isActive = session.status !== "idle" && session.status !== "done";

  return (
    <Box
      flexDirection="column"
      borderStyle="single"
      paddingX={1}
      borderColor={isActive ? "red" : undefined}
    >
      <Text bold color="yellow">
        Close session at "{session.path}"
      </Text>

      {isActive && (
        <Box marginTop={1}>
          <Text color="red">
            Claude is actively running ({session.status}). It will be terminated.
          </Text>
        </Box>
      )}

      <Box flexDirection="column" marginTop={1}>
        <Text>Remove this session from the dashboard?</Text>
        <Box marginTop={1}>
          <Text>
            <Text color="yellow">[Enter/y]</Text> Yes{" "}
            <Text color="yellow">[n/Esc]</Text> Cancel
          </Text>
        </Box>
      </Box>
    </Box>
  );
}
