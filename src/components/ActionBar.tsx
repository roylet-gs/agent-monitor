import React from "react";
import { Box, Text } from "ink";
import { Spinner } from "./Spinner.js";

interface ActionBarProps {
  busy: string | null;
  hasWorktrees: boolean;
  escHint: boolean;
}

export function ActionBar({ busy, hasWorktrees, escHint }: ActionBarProps) {
  if (busy) {
    return (
      <Box borderStyle="single" borderTop borderBottom={false} borderLeft={false} borderRight={false} paddingX={1}>
        <Spinner label={busy} />
      </Box>
    );
  }

  if (escHint) {
    return (
      <Box borderStyle="single" borderTop borderBottom={false} borderLeft={false} borderRight={false} paddingX={1}>
        <Text dimColor>Press </Text>
        <Text color="yellow">Esc</Text>
        <Text dimColor> again to quit</Text>
      </Box>
    );
  }

  return (
    <Box borderStyle="single" borderTop borderBottom={false} borderLeft={false} borderRight={false} paddingX={1}>
      {hasWorktrees ? (
        <Text>
          <Text color="yellow">[Enter]</Text> Open
          <Text color="yellow"> [n]</Text>ew
          <Text color="yellow"> [d]</Text>elete
          <Text color="yellow"> [g]</Text>ithub
          <Text color="yellow"> [l]</Text>inear
          <Text color="yellow"> [r]</Text>efresh
          <Text color="yellow"> [w]</Text>atch
          <Text color="yellow"> [s]</Text>ettings
          <Text color="yellow"> [q]</Text>uit
        </Text>
      ) : (
        <Text>
          <Text color="yellow">[n]</Text>ew
          <Text color="yellow"> [r]</Text>efresh
          <Text color="yellow"> [s]</Text>ettings
          <Text color="yellow"> [q]</Text>uit
        </Text>
      )}
    </Box>
  );
}
