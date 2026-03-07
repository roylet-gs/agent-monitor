import React from "react";
import { Box, Text } from "ink";
import type { PaneState } from "../hooks/useTerminalPanes.js";

interface TerminalPaneProps {
  pane: PaneState;
  height: number;
}

export function TerminalPane({ pane, height }: TerminalPaneProps) {
  const borderColor = pane.focused ? "cyan" : "gray";

  // Lines are a full screen snapshot from xterm-headless (rows match pane height)
  const visibleLines = pane.lines;

  // Build title
  let title = pane.title;
  if (pane.role) title += ` [${pane.role}]`;
  if (pane.exited) title += " [exited]";

  return (
    <Box
      flexDirection="column"
      flexGrow={1}
      height={height}
      borderStyle="single"
      borderColor={borderColor}
    >
      <Box>
        <Text color={borderColor} bold={pane.focused}>
          {title}
        </Text>
      </Box>
      <Box flexDirection="column" flexGrow={1}>
        {visibleLines.map((line, i) => (
          <Text key={i} wrap="truncate">
            {line}
          </Text>
        ))}
      </Box>
    </Box>
  );
}
