import React, { useState } from "react";
import { Box, Text, useInput } from "ink";
import { homedir } from "os";
import type { Repository } from "../lib/types.js";

interface RepoSelectorProps {
  repositories: Repository[];
  onSelect: (repo: Repository) => void;
  onCancel: () => void;
}

export function RepoSelector({
  repositories,
  onSelect,
  onCancel,
}: RepoSelectorProps) {
  const [selectedIndex, setSelectedIndex] = useState(0);

  useInput((input, key) => {
    if (key.escape) {
      onCancel();
      return;
    }

    if (key.upArrow || input === "k") {
      setSelectedIndex((i) => Math.max(0, i - 1));
      return;
    }

    if (key.downArrow || input === "j") {
      setSelectedIndex((i) => Math.min(repositories.length - 1, i + 1));
      return;
    }

    if (key.return) {
      const repo = repositories[selectedIndex];
      if (repo) onSelect(repo);
    }
  });

  return (
    <Box flexDirection="column" borderStyle="single" paddingX={1}>
      <Text bold color="cyan">
        Switch Repository
      </Text>

      <Box flexDirection="column" marginTop={1}>
        {repositories.map((repo, i) => {
          const displayPath = repo.path.replace(homedir(), "~");
          return (
            <Box key={repo.id} gap={2}>
              <Text>{i === selectedIndex ? "▸" : " "}</Text>
              <Text
                bold={i === selectedIndex}
                color={i === selectedIndex ? "cyan" : undefined}
              >
                {repo.name}
              </Text>
              <Text dimColor>{displayPath}</Text>
            </Box>
          );
        })}
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
          <Text color="yellow">[Enter]</Text> Select{" "}
          <Text color="yellow">[↑↓]</Text> Navigate{" "}
          <Text color="yellow">[Esc]</Text> Cancel
        </Text>
      </Box>
    </Box>
  );
}
