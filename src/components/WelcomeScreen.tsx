import React from "react";
import { Box, Text, useInput } from "ink";
import type { ReleaseNote } from "../lib/version.js";

interface WelcomeScreenProps {
  version: string;
  releaseNotes: ReleaseNote[];
  onDismiss: () => void;
}

export function WelcomeScreen({ version, releaseNotes, onDismiss }: WelcomeScreenProps) {
  useInput(() => {
    onDismiss();
  });

  return (
    <Box flexDirection="column" paddingX={2} paddingY={1}>
      <Box marginBottom={1}>
        <Text bold color="cyan">
          Agent Monitor{" "}
        </Text>
        <Text dimColor>v{version}</Text>
      </Box>

      <Box flexDirection="column">
        <Text bold color="green">Updated to v{version}</Text>
        {releaseNotes.length > 0 && (
          <Box flexDirection="column" marginTop={1}>
            <Text bold>What's new:</Text>
            {releaseNotes.map((note) => (
              <Box key={note.hash} marginLeft={1}>
                <Text dimColor>{note.hash} </Text>
                <Text>{note.message}</Text>
              </Box>
            ))}
          </Box>
        )}
      </Box>

      <Box marginTop={1}>
        <Text dimColor>Press any key to continue...</Text>
      </Box>
    </Box>
  );
}
