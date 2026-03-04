import React from "react";
import { ProgressSteps, type StepInfo } from "./ProgressSteps.js";

export type { StepInfo };

interface CreatingWorktreeProps {
  branchName: string;
  steps: StepInfo[];
  error?: string | null;
}

export function CreatingWorktree({ branchName, steps, error }: CreatingWorktreeProps) {
  return (
    <ProgressSteps
      title="New Worktree"
      subtitle={`Creating ${branchName}...`}
      steps={steps}
      error={error}
    />
  );
}
