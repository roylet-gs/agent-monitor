import React, { useState } from "react";
import { Box, Text, useInput } from "ink";
import TextInput from "ink-text-input";
import type { PendingInput } from "../lib/types.js";

interface InputPromptProps {
  /** For respond-input mode: the pending input to respond to */
  pendingInput?: PendingInput | null;
  /** For send-prompt mode: compose a new prompt */
  composeMode?: boolean;
  /** Worktree branch name for display */
  branchName?: string;
  onSubmit: (response: string, decision?: "allow" | "deny") => void;
  onCancel: () => void;
}

export function InputPrompt({ pendingInput, composeMode, branchName, onSubmit, onCancel }: InputPromptProps) {
  const [input, setInput] = useState("");

  useInput((ch, key) => {
    if (key.escape) {
      onCancel();
      return;
    }

    // Permission shortcuts
    if (pendingInput?.type === "permission" && !input) {
      if (ch === "a" || ch === "y") {
        onSubmit("", "allow");
        return;
      }
      if (ch === "x") {
        onSubmit("", "deny");
        return;
      }
    }
  });

  const handleSubmit = (value: string) => {
    if (!value.trim() && !composeMode) return;
    if (pendingInput?.type === "permission") {
      onSubmit(value, "allow");
    } else {
      onSubmit(value);
    }
  };

  if (composeMode) {
    return (
      <Box
        flexDirection="column"
        borderStyle="single"
        flexGrow={1}
        paddingX={1}
      >
        <Text bold color="cyan"> Send Message {branchName ? `to ${branchName}` : ""}</Text>
        <Box marginTop={1} flexDirection="column">
          <Text dimColor>Type your message and press Enter to send, or Esc to cancel.</Text>
          <Box marginTop={1}>
            <Text color="yellow">&gt; </Text>
            <TextInput value={input} onChange={setInput} onSubmit={handleSubmit} />
          </Box>
        </Box>
      </Box>
    );
  }

  if (!pendingInput) return null;

  const isPermission = pendingInput.type === "permission";

  return (
    <Box
      flexDirection="column"
      borderStyle="single"
      flexGrow={1}
      paddingX={1}
    >
      <Text bold color={isPermission ? "yellow" : "cyan"}>
        {isPermission ? " Permission Request" : " Question from Claude"}
        {branchName ? ` — ${branchName}` : ""}
      </Text>

      <Box marginTop={1} flexDirection="column">
        {/* Question/permission text */}
        <Text>{pendingInput.question}</Text>

        {/* Tool info for permissions */}
        {isPermission && pendingInput.toolName && (
          <Box marginTop={1}>
            <Text dimColor>Tool: </Text>
            <Text color="yellow">{pendingInput.toolName}</Text>
          </Box>
        )}

        {/* Options for questions */}
        {!isPermission && pendingInput.options && pendingInput.options.length > 0 && (
          <Box marginTop={1} flexDirection="column">
            <Text dimColor>Options:</Text>
            {pendingInput.options.map((opt, i) => (
              <Text key={i}>
                <Text dimColor>  {i + 1}. </Text>
                <Text>{opt.label}</Text>
                {opt.description && <Text dimColor> — {opt.description}</Text>}
              </Text>
            ))}
          </Box>
        )}

        {/* Input area */}
        <Box marginTop={1}>
          {isPermission ? (
            <Text>
              <Text color="green">[a]</Text>pprove
              {"  "}
              <Text color="red">[x]</Text> deny
              {"  "}
              <Text dimColor>[Esc] cancel</Text>
            </Text>
          ) : (
            <Box flexDirection="column">
              <Text dimColor>Type your answer and press Enter, or Esc to cancel.</Text>
              <Box marginTop={1}>
                <Text color="yellow">&gt; </Text>
                <TextInput value={input} onChange={setInput} onSubmit={handleSubmit} />
              </Box>
            </Box>
          )}
        </Box>
      </Box>
    </Box>
  );
}
