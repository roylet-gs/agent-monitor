import React from "react";
import { Box, Text } from "ink";

interface TerminalActionBarProps {
  commandMode: boolean;
  showHelp: boolean;
}

export function TerminalActionBar({ commandMode, showHelp }: TerminalActionBarProps) {
  if (showHelp) {
    return (
      <Box flexDirection="column" borderStyle="single" borderColor="cyan" paddingX={1}>
        <Text bold color="cyan">Terminal Commands (after Ctrl+A)</Text>
        <Text>  <Text color="yellow">c</Text>  Create new pane</Text>
        <Text>  <Text color="yellow">x</Text>  Close focused pane</Text>
        <Text>  <Text color="yellow">h/l</Text>  Focus prev/next pane</Text>
        <Text>  <Text color="yellow">d</Text>  Detach (return to dashboard)</Text>
        <Text>  <Text color="yellow">z</Text>  Zoom/fullscreen toggle</Text>
        <Text>  <Text color="yellow">?</Text>  Toggle this help</Text>
        <Text dimColor>  Press any key to dismiss</Text>
      </Box>
    );
  }

  return (
    <Box>
      {commandMode ? (
        <Text>
          <Text color="cyan" bold> CMD </Text>{" "}
          <Text color="yellow">[c]</Text>reate{" "}
          <Text color="yellow">[x]</Text>close{" "}
          <Text color="yellow">[h/l]</Text>focus{" "}
          <Text color="yellow">[d]</Text>etach{" "}
          <Text color="yellow">[z]</Text>oom{" "}
          <Text color="yellow">[?]</Text>help
        </Text>
      ) : (
        <Text dimColor>
          {" "}^A: command mode | Type to interact with agent
        </Text>
      )}
    </Box>
  );
}
