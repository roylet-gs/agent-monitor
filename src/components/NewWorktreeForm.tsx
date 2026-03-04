import React, { useState, useRef } from "react";
import { Box, Text, useInput } from "ink";
import TextInput from "ink-text-input";

interface NewWorktreeFormProps {
  defaultPrefix: string;
  onSubmit: (branchName: string, customName: string) => void;
  onCancel: () => void;
}

export function NewWorktreeForm({
  defaultPrefix,
  onSubmit,
  onCancel,
}: NewWorktreeFormProps) {
  const [activeField, setActiveField] = useState<"branch" | "name">("branch");
  const [branchName, setBranchName] = useState(defaultPrefix);
  const [customName, setCustomName] = useState("");
  const submittedRef = useRef(false);

  const doSubmit = () => {
    if (submittedRef.current) return;
    if (!branchName.trim()) return;
    submittedRef.current = true;
    onSubmit(branchName.trim(), customName.trim());
  };

  useInput((_input, key) => {
    if (key.escape) {
      onCancel();
      return;
    }

    if (key.tab) {
      setActiveField((f) => (f === "branch" ? "name" : "branch"));
      return;
    }
  });

  return (
    <Box flexDirection="column" borderStyle="single" paddingX={1}>
      <Text bold color="cyan">
        New Worktree
      </Text>

      <Box flexDirection="column" marginTop={1} gap={1}>
        <Box>
          <Text bold={activeField === "branch"}>
            Branch name:{" "}
          </Text>
          {activeField === "branch" ? (
            <TextInput
              value={branchName}
              onChange={setBranchName}
              onSubmit={() => setActiveField("name")}
            />
          ) : (
            <Text>{branchName || <Text dimColor>(empty)</Text>}</Text>
          )}
        </Box>

        <Box>
          <Text bold={activeField === "name"}>
            Name (optional):{" "}
          </Text>
          {activeField === "name" ? (
            <TextInput
              value={customName}
              onChange={setCustomName}
              onSubmit={doSubmit}
            />
          ) : (
            <Text>{customName || <Text dimColor>(empty)</Text>}</Text>
          )}
        </Box>
      </Box>

      <Box
        marginTop={1}
        borderStyle="single"
        borderTop
        borderBottom={false}
        borderLeft={false}
        borderRight={false}
      >
        <Text>
          <Text color="yellow">[Tab]</Text> Next field{" "}
          <Text color="yellow">[Enter]</Text> Create{" "}
          <Text color="yellow">[Esc]</Text> Cancel
        </Text>
      </Box>
    </Box>
  );
}
