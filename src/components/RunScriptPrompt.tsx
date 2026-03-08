import React, { useState, useEffect } from "react";
import { Box, Text, useInput } from "ink";

interface RunScriptPromptProps {
  scriptPath: string;
  onRun: () => void;
  onSkip: () => void;
}

export function RunScriptPrompt({
  scriptPath,
  onRun,
  onSkip,
}: RunScriptPromptProps) {
  // Ignore input on the first frame to avoid the Enter keypress
  // that submitted the previous form from bleeding through
  const [ready, setReady] = useState(false);
  useEffect(() => {
    const timer = setTimeout(() => setReady(true), 50);
    return () => clearTimeout(timer);
  }, []);

  useInput((input, key) => {
    if (!ready) return;
    if (key.escape || input === "n") {
      onSkip();
      return;
    }
    if (key.return || input === "y") {
      onRun();
    }
  });

  return (
    <Box flexDirection="column" borderStyle="single" paddingX={1} borderColor="green">
      <Text bold color="green">
        Run startup script?
      </Text>
      <Box marginTop={1} flexDirection="column">
        <Text>A startup script exists for this repository:</Text>
        <Text dimColor>{scriptPath}</Text>
      </Box>
      <Box marginTop={1} flexDirection="column">
        <Text>
          <Text color="green">[Enter/y]</Text> Run script
        </Text>
        <Text>
          <Text color="yellow">[Esc/n]</Text> Skip
        </Text>
      </Box>
    </Box>
  );
}
