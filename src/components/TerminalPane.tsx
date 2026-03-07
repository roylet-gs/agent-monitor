import React from "react";
import { Box, Text } from "ink";
import type { PaneState } from "../hooks/useTerminalPanes.js";

interface TerminalPaneProps {
  pane: PaneState;
  width: number;
  height: number;
}

export function TerminalPane({ pane, width, height }: TerminalPaneProps) {
  const borderColor = pane.focused ? "cyan" : "gray";
  const contentHeight = Math.max(height - 2, 1); // account for borders

  // Get the last N lines that fit in the pane
  const visibleLines = pane.lines.slice(-contentHeight);

  // Build title
  let title = pane.title;
  if (pane.role) title += ` [${pane.role}]`;
  if (pane.exited) title += " [exited]";

  // Truncate title to fit width
  const maxTitleLen = Math.max(width - 4, 5);
  if (title.length > maxTitleLen) {
    title = title.slice(0, maxTitleLen - 1) + "\u2026";
  }

  return (
    <Box
      flexDirection="column"
      width={width}
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
