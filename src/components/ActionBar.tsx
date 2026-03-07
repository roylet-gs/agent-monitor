import React from "react";
import { Box, Text } from "ink";
import { Spinner } from "./Spinner.js";

interface ActionBarProps {
  busy: string | null;
  hasWorktrees: boolean;
  escHint: boolean;
  ghPrStatus?: boolean;
  linearEnabled?: boolean;
}

export function ActionBar({ busy, hasWorktrees, escHint, ghPrStatus, linearEnabled }: ActionBarProps) {
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
          <Text color="yellow">[Enter]</Text><Text dimColor>Open </Text>
          <Text color="yellow">[n]</Text><Text dimColor>ew </Text>
          <Text color="yellow">[d]</Text><Text dimColor>elete </Text>
          <Text color="yellow">[t]</Text><Text dimColor>erminal </Text>
          <Text color="yellow">[c]</Text><Text dimColor>laude </Text>
          {ghPrStatus && <><Text color="yellow">[g]</Text><Text dimColor>ithub </Text></>}
          {linearEnabled && <><Text color="yellow">[l]</Text><Text dimColor>inear </Text></>}
          <Text color="yellow">[r]</Text><Text dimColor>efresh </Text>
          <Text color="yellow">[w]</Text><Text dimColor>atch </Text>
          <Text color="yellow">[s]</Text><Text dimColor>ettings </Text>
          <Text color="yellow">[q]</Text><Text dimColor>uit </Text>
          <Text color="yellow">[j/k]</Text><Text dimColor>nav</Text>
        </Text>
      ) : (
        <Text>
          <Text color="yellow">[n]</Text><Text dimColor>ew </Text>
          <Text color="yellow">[r]</Text><Text dimColor>efresh </Text>
          <Text color="yellow">[s]</Text><Text dimColor>ettings </Text>
          <Text color="yellow">[q]</Text><Text dimColor>uit</Text>
        </Text>
      )}
    </Box>
  );
}
