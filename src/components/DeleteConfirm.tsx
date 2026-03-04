import React, { useState, useEffect } from "react";
import { Box, Text, useInput } from "ink";
import type { WorktreeWithStatus } from "../lib/types.js";
import { remoteBranchExists } from "../lib/git.js";

export interface DeleteOptions {
  deleteLocalBranch: boolean;
  deleteRemoteBranch: boolean;
}

interface DeleteConfirmProps {
  worktree: WorktreeWithStatus;
  repoPath: string;
  onConfirm: (options: DeleteOptions) => void;
  onCancel: () => void;
}

type Step = "confirm" | "local-branch" | "remote-branch";

export function DeleteConfirm({
  worktree,
  repoPath,
  onConfirm,
  onCancel,
}: DeleteConfirmProps) {
  const [step, setStep] = useState<Step>("confirm");
  const [deleteLocal, setDeleteLocal] = useState(false);
  const [hasRemote, setHasRemote] = useState<boolean | null>(null);

  // Check if remote branch exists on mount
  useEffect(() => {
    remoteBranchExists(repoPath, worktree.branch).then(setHasRemote);
  }, [repoPath, worktree.branch]);

  useInput((input, key) => {
    if (key.escape) {
      onCancel();
      return;
    }

    if (step === "confirm") {
      if (input === "n") {
        onCancel();
      } else if (key.return || input === "y") {
        setStep("local-branch");
      }
      return;
    }

    if (step === "local-branch") {
      if (key.return || input === "y") {
        setDeleteLocal(true);
        if (hasRemote) {
          setStep("remote-branch");
        } else {
          onConfirm({ deleteLocalBranch: true, deleteRemoteBranch: false });
        }
      } else if (input === "n") {
        setDeleteLocal(false);
        if (hasRemote) {
          setStep("remote-branch");
        } else {
          onConfirm({ deleteLocalBranch: false, deleteRemoteBranch: false });
        }
      }
      return;
    }

    if (step === "remote-branch") {
      if (key.return || input === "y") {
        onConfirm({ deleteLocalBranch: deleteLocal, deleteRemoteBranch: true });
      } else if (input === "n") {
        onConfirm({ deleteLocalBranch: deleteLocal, deleteRemoteBranch: false });
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
        Delete worktree "{name}"
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
          <Text>Remove this worktree from disk?</Text>
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

      {step === "remote-branch" && (
        <Box flexDirection="column" marginTop={1}>
          <Text>
            Also delete remote branch <Text bold>origin/{worktree.branch}</Text>?
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
