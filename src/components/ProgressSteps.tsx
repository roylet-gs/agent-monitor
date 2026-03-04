import React from "react";
import { Box, Text } from "ink";
import { Spinner } from "./Spinner.js";

export interface StepInfo {
  label: string;
  status: "pending" | "active" | "done" | "error";
}

interface ProgressStepsProps {
  title: string;
  subtitle: string;
  steps: StepInfo[];
  error?: string | null;
}

export function ProgressSteps({ title, subtitle, steps, error }: ProgressStepsProps) {
  return (
    <Box flexDirection="column" borderStyle="single" paddingX={1}>
      <Text bold color="cyan">
        {title}
      </Text>

      <Box marginTop={1}>
        <Text>{subtitle}</Text>
      </Box>

      <Box flexDirection="column" marginTop={1}>
        {steps.map((step, i) => (
          <Box key={i}>
            {step.status === "active" && <Spinner />}
            {step.status === "done" && <Text color="green">✓</Text>}
            {step.status === "pending" && <Text dimColor> </Text>}
            {step.status === "error" && <Text color="red">✗</Text>}
            <Text dimColor={step.status === "pending"}>
              {" "}{step.label}
            </Text>
          </Box>
        ))}
      </Box>

      {error && (
        <Box marginTop={1}>
          <Text color="red">Error: {error}</Text>
        </Box>
      )}
    </Box>
  );
}
