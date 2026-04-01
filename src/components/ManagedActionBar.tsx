import React from "react";
import { Box, Text } from "ink";
import type { PendingInput } from "../lib/types.js";

interface ManagedActionBarProps {
  pendingInput: PendingInput | null;
  canMessage: boolean;
  pendingCount: number;
}

export const ManagedActionBar = React.memo(function ManagedActionBar({
  pendingInput,
  canMessage,
  pendingCount,
}: ManagedActionBarProps) {
  if (!pendingInput && !canMessage) return null;

  return (
    <Box paddingX={1}>
      {pendingInput?.type === "permission" && (
        <Text>
          <Text color="yellow" bold>⚡ Permission: </Text>
          <Text>{pendingInput.toolName ?? "tool"} </Text>
          <Text color="green">[a]</Text><Text>pprove </Text>
          <Text color="red">[x]</Text><Text> deny </Text>
          <Text color="yellow">[Enter]</Text><Text> approve</Text>
          {pendingCount > 1 && <Text dimColor>  (+{pendingCount - 1} more)</Text>}
        </Text>
      )}

      {pendingInput?.type === "question" && (
        <Text>
          <Text color="cyan" bold>? Question: </Text>
          <Text wrap="truncate-end">{pendingInput.question?.slice(0, 60)} </Text>
          <Text color="yellow">[Enter]</Text><Text> respond</Text>
          {pendingCount > 1 && <Text dimColor>  (+{pendingCount - 1} more)</Text>}
        </Text>
      )}

      {!pendingInput && canMessage && (
        <Text>
          <Text dimColor bold>💬 Agent idle </Text>
          <Text color="yellow">[m]</Text><Text>essage</Text>
        </Text>
      )}
    </Box>
  );
});
