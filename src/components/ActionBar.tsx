import React from "react";
import { Box, Text } from "ink";
import { Spinner } from "./Spinner.js";

interface ActionBarProps {
  busy: string | null;
  hasWorktrees: boolean;
  escHint: boolean;
  ghPrStatus?: boolean;
  linearEnabled?: boolean;
  hasPr?: boolean;
  hasLinear?: boolean;
  hasLinearProject?: boolean;
  ideIsTerm?: boolean;
  integrationLoading?: string | null;
  /** Chat pane has focus — show chat keys instead of dashboard keys. */
  chatMode?: boolean;
}

export const ActionBar = React.memo(function ActionBar({ busy, hasWorktrees, escHint, ghPrStatus, linearEnabled, hasPr, hasLinear, hasLinearProject, ideIsTerm, integrationLoading, chatMode }: ActionBarProps) {
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

  if (chatMode) {
    return (
      <Box borderStyle="single" borderTop borderBottom={false} borderLeft={false} borderRight={false} paddingX={1}>
        <Text>
          <Text color="yellow">[Enter]</Text> Send
          <Text color="yellow"> [↑↓]</Text> Scroll
          <Text color="yellow"> [Tab]</Text> Terminal
          <Text color="yellow"> [Shift+Tab]</Text> IDE
          <Text color="yellow"> [Esc]</Text> Back
        </Text>
      </Box>
    );
  }

  return (
    <Box borderStyle="single" borderTop borderBottom={false} borderLeft={false} borderRight={false} paddingX={1}>
      {integrationLoading && <><Spinner label={integrationLoading} /><Text> </Text></>}
      {hasWorktrees ? (
        <Text>
          <Text color="yellow">[Enter]</Text> Open
          {!ideIsTerm && <><Text color="yellow"> [t]</Text>erminal</>}
          <Text color="yellow"> [c]</Text>hat
          <Text color="yellow"> [n]</Text>ew
          <Text color="yellow"> [d]</Text>elete
          {(ghPrStatus || linearEnabled) && (
            hasPr ? <><Text color="yellow"> [g]</Text>ithub</> : <Text dimColor> [g]ithub</Text>
          )}
          {linearEnabled && (
            hasLinear ? <><Text color="yellow"> [l]</Text>inear</> : <Text dimColor> [l]inear</Text>
          )}
          {linearEnabled && (
            hasLinearProject ? <><Text color="yellow"> [p]</Text>roject</> : <Text dimColor> [p]roject</Text>
          )}
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
});
