import React from "react";
import { Box, Text } from "ink";
import { useLogTail } from "../hooks/useLogTail.js";

interface LogPanelProps {
  height: number;
}

function colorForLevel(line: string): string | undefined {
  if (line.includes("[ERROR]")) return "red";
  if (line.includes("[WARN]")) return "yellow";
  if (line.includes("[DEBUG]")) return undefined; // handled via dimColor
  return undefined;
}

function isDimLine(line: string): boolean {
  return line.includes("[DEBUG]");
}

export function LogPanel({ height }: LogPanelProps) {
  const lines = useLogTail(true, height - 2);

  return (
    <Box
      flexDirection="column"
      height={height}
      borderStyle="single"
      borderBottom={false}
      borderLeft={false}
      borderRight={false}
    >
      <Box>
        <Text bold color="cyan"> Logs </Text>
        <Text dimColor>(w to hide)</Text>
      </Box>
      {lines.map((line, i) => (
        <Text key={i} wrap="truncate-end" color={colorForLevel(line)} dimColor={isDimLine(line)}>
          {line}
        </Text>
      ))}
    </Box>
  );
}
