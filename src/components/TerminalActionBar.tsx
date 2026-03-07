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
        <Text bold color="cyan">Terminal Commands (press Esc to enter command mode)</Text>
        <Text>  <Text color="yellow">C</Text>         Create new pane</Text>
        <Text>  <Text color="yellow">W</Text>         Close focused pane</Text>
        <Text>  <Text color="yellow">H / Left</Text>  Focus prev pane</Text>
        <Text>  <Text color="yellow">L / Right</Text> Focus next pane</Text>
        <Text>  <Text color="yellow">1-4</Text>       Focus pane by number</Text>
        <Text>  <Text color="yellow">D</Text>         Detach (back to dashboard)</Text>
        <Text>  <Text color="yellow">Z</Text>         Zoom/fullscreen toggle</Text>
        <Text>  <Text color="yellow">?</Text>         Toggle this help</Text>
        <Text>  <Text color="yellow">Esc</Text>       Detach (back to dashboard)</Text>
        <Text dimColor>  Mouse: click a pane to focus it</Text>
      </Box>
    );
  }

  return (
    <Box>
      {commandMode ? (
        <Text>
          <Text color="cyan" bold> [CMD] </Text>{" "}
          <Text color="yellow">[C]</Text>reate{" "}
          <Text color="yellow">[W]</Text>close{" "}
          <Text color="yellow">[H/L]</Text>focus{" "}
          <Text color="yellow">[1-4]</Text>pane{" "}
          <Text color="yellow">[D]</Text>etach{" "}
          <Text color="yellow">[Z]</Text>oom{" "}
          <Text color="yellow">[?]</Text>help{" "}
          <Text color="yellow">[Esc]</Text>detach
        </Text>
      ) : (
        <Text>
          <Text dimColor> Esc: command mode | Click pane to focus</Text>
        </Text>
      )}
    </Box>
  );
}
