import React, { useState } from "react";
import { Box, Text, useInput } from "ink";
import type { WorktreeWithStatus } from "../lib/types.js";

export interface DeleteOptions {
  deleteLocalBranch: boolean;
  deleteRemoteBranch: boolean;
  isBranchOnly?: boolean;
}

interface DeleteConfirmProps {
  worktree: WorktreeWithStatus;
  repoPath: string;
  onConfirm: (options: DeleteOptions) => void;
  onCancel: () => void;
}

type Step = "confirm" | "local-branch";

export function DeleteConfirm({
  worktree,
  onConfirm,
  onCancel,
}: DeleteConfirmProps) {
  const isBranchOnly = worktree.is_main === 1 && worktree.branch !== "main" && worktree.branch !== "master";
  const [step, setStep] = useState<Step>("confirm");

  useInput((input, key) => {
    if (key.escape) {
      onCancel();
      return;
    }

    if (step === "confirm") {
      if (input === "n") {
        onCancel();
      } else if (key.return || input === "y") {
        if (isBranchOnly) {
          onConfirm({ deleteLocalBranch: true, deleteRemoteBranch: false, isBranchOnly: true });
        } else {
          setStep("local-branch");
        }
      }
      return;
    }

    if (step === "local-branch") {
      if (key.return || input === "y") {
        onConfirm({ deleteLocalBranch: true, deleteRemoteBranch: false });
      } else if (input === "n") {
        onConfirm({ deleteLocalBranch: false, deleteRemoteBranch: false });
      }
      return;
    }
  });

  const dirty = worktree.git_status?.dirty ?? 0;
  const ahead = worktree.git_status?.ahead ?? 0;
  const hasWarnings = dirty > 0 || ahead > 0;
  const name = worktree.custom_name ?? worktree.branch;

  return (
    <Box
      flexDirection="column"
      borderStyle="single"
      paddingX={1}
      borderColor={hasWarnings ? "yellow" : undefined}
    >
      <Text bold color="yellow">
        {isBranchOnly ? `Delete branch "${name}"` : `Delete worktree "${name}"`}
      </Text>

      {hasWarnings && (
        <Box flexDirection="column" marginTop={1}>
          {dirty > 0 && (
            <Text color="yellow">
              {"  "} {dirty} uncommitted change{dirty !== 1 ? "s" : ""}
            </Text>
          )}
          {ahead > 0 && (
            <Text color="yellow">
              {"  "} {ahead} commit{ahead !== 1 ? "s" : ""} not pushed to remote
            </Text>
          )}
        </Box>
      )}

      {step === "confirm" && (
        <Box flexDirection="column" marginTop={1}>
          {isBranchOnly ? (
            <Text>Delete branch <Text bold>{worktree.branch}</Text> and switch back to the default branch?</Text>
          ) : (
            <Text>Remove this worktree from disk?</Text>
          )}
          <Box marginTop={1}>
            <Text>
              <Text color="yellow">[Enter/y]</Text> Yes{" "}
              <Text color="yellow">[n/Esc]</Text> Cancel
            </Text>
          </Box>
        </Box>
      )}

      {step === "local-branch" && (
        <Box flexDirection="column" marginTop={1}>
          <Text>
            Also delete local branch <Text bold>{worktree.branch}</Text>?
          </Text>
          <Box marginTop={1}>
            <Text>
              <Text color="yellow">[Enter/y]</Text> Yes{" "}
              <Text color="yellow">[n]</Text> No, keep it
            </Text>
          </Box>
        </Box>
      )}
    </Box>
  );
}
