import React from "react";
import { Box, Text } from "ink";
import type { PaneState } from "../hooks/useTerminalPanes.js";

interface TerminalTabBarProps {
  panes: PaneState[];
}

export function TerminalTabBar({ panes }: TerminalTabBarProps) {
  return (
    <Box>
      <Text dimColor> Panes: </Text>
      {panes.map((pane, i) => (
        <Box key={pane.id} marginRight={1}>
          <Text
            color={pane.focused ? "cyan" : "gray"}
            bold={pane.focused}
          >
            {i + 1}:{pane.title}
            {pane.focused ? "*" : ""}
            {pane.exited ? " [x]" : ""}
          </Text>
        </Box>
      ))}
    </Box>
  );
}
