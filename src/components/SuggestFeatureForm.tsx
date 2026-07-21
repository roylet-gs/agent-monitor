import React, { useState, useRef } from "react";
import { Box, Text, useInput } from "ink";
import TextInput from "ink-text-input";

interface SuggestFeatureFormProps {
  onSubmit: (title: string, description: string) => void;
  onCancel: () => void;
}

export function SuggestFeatureForm({ onSubmit, onCancel }: SuggestFeatureFormProps) {
  const [activeField, setActiveField] = useState<"title" | "description">("title");
  const [title, setTitle] = useState("");
  const [description, setDescription] = useState("");
  const submittedRef = useRef(false);

  const doSubmit = () => {
    if (submittedRef.current) return;
    if (!title.trim()) return;
    submittedRef.current = true;
    onSubmit(title.trim(), description.trim());
  };

  useInput((_input, key) => {
    if (key.escape) {
      onCancel();
      return;
    }

    if (key.tab) {
      setActiveField((f) => (f === "title" ? "description" : "title"));
      return;
    }
  });

  return (
    <Box flexDirection="column" borderStyle="single" paddingX={1}>
      <Text bold color="cyan">
        Suggest a Feature
      </Text>

      <Box flexDirection="column" marginTop={1} gap={1}>
        <Box>
          <Text bold={activeField === "title"}>
            Title:{" "}
          </Text>
          {activeField === "title" ? (
            <TextInput
              value={title}
              onChange={setTitle}
              onSubmit={() => setActiveField("description")}
            />
          ) : (
            <Text>{title || <Text dimColor>(required)</Text>}</Text>
          )}
        </Box>

        <Box>
          <Text bold={activeField === "description"}>
            Description (optional):{" "}
          </Text>
          {activeField === "description" ? (
            <TextInput
              value={description}
              onChange={setDescription}
              onSubmit={doSubmit}
            />
          ) : (
            <Text>{description || <Text dimColor>(empty)</Text>}</Text>
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
          <Text color="yellow">[Enter]</Text> Submit{" "}
          <Text color="yellow">[Esc]</Text> Cancel
        </Text>
      </Box>
    </Box>
  );
}
