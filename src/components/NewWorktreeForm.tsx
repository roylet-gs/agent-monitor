import React, { useState, useRef, useEffect } from "react";
import { Box, Text, useInput } from "ink";
import TextInput from "ink-text-input";
import { log } from "../lib/logger.js";
import type { BranchCheckResult } from "../lib/types.js";

interface NewWorktreeFormProps {
  defaultPrefix: string;
  defaultBaseBranch: string;
  onSubmit: (branchName: string, customName: string, baseBranch: string) => void;
  onCancel: () => void;
  /** Optional advisory check while typing; absent means no live indicator. */
  checkBranch?: (branch: string) => Promise<BranchCheckResult>;
  /** Debounce for the live check in ms. */
  checkDebounceMs?: number;
}

type CheckState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "result"; branch: string; local: boolean; remote: boolean };

export function NewWorktreeForm({
  defaultPrefix,
  defaultBaseBranch,
  onSubmit,
  onCancel,
  checkBranch,
  checkDebounceMs = 400,
}: NewWorktreeFormProps) {
  const [activeField, setActiveField] = useState<"branch" | "name" | "baseBranch">("branch");
  const [branchName, setBranchName] = useState(defaultPrefix);
  const [customName, setCustomName] = useState("");
  const [baseBranch, setBaseBranch] = useState(defaultBaseBranch);
  const [check, setCheck] = useState<CheckState>({ kind: "idle" });
  const submittedRef = useRef(false);
  const checkSeq = useRef(0);

  const trimmed = branchName.trim();
  // Don't check empty input, the untouched default prefix, or a bare prefix.
  const checkable =
    trimmed.length > 0 && trimmed !== defaultPrefix.trim() && !trimmed.endsWith("/");

  useEffect(() => {
    // Bump the sequence even when skipping so an in-flight result can't
    // resurrect after the user clears the field.
    const seq = ++checkSeq.current;
    if (!checkBranch || !checkable) {
      setCheck({ kind: "idle" });
      return;
    }
    setCheck({ kind: "checking" });
    const timer = setTimeout(() => {
      checkBranch(trimmed).then(
        (res) => {
          if (seq !== checkSeq.current) return;
          setCheck({ kind: "result", branch: trimmed, local: res.local, remote: res.remote });
        },
        (err) => {
          if (seq !== checkSeq.current) return;
          log("debug", "ui", `Live branch check failed for ${trimmed}: ${err}`);
          setCheck({ kind: "idle" });
        }
      );
    }, checkDebounceMs);
    return () => clearTimeout(timer);
  }, [trimmed, checkable, checkBranch, checkDebounceMs]);

  // The live check is advisory only — doCreateWorktree re-verifies on submit.
  const remoteConfirmed =
    check.kind === "result" && check.remote && check.branch === trimmed;
  const localOnly =
    check.kind === "result" && !check.remote && check.local && check.branch === trimmed;

  const doSubmit = () => {
    if (submittedRef.current) return;
    if (!branchName.trim()) return;
    submittedRef.current = true;
    onSubmit(branchName.trim(), customName.trim(), baseBranch.trim());
  };

  useInput((_input, key) => {
    if (key.escape) {
      onCancel();
      return;
    }

    if (key.tab) {
      setActiveField((f) => {
        if (f === "branch") return "name";
        if (f === "name") return "baseBranch";
        return "branch";
      });
      return;
    }

    // When the base field is replaced by the "(ignored)" note there is no
    // TextInput to receive Enter, so submit from here.
    if (key.return && remoteConfirmed && activeField === "baseBranch") {
      doSubmit();
      return;
    }
  });

  return (
    <Box flexDirection="column" borderStyle="single" paddingX={1}>
      <Text bold color="cyan">
        New Worktree
      </Text>

      <Box flexDirection="column" marginTop={1} gap={1}>
        <Box flexDirection="column">
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
          {check.kind === "checking" && <Text dimColor>  checking origin…</Text>}
          {remoteConfirmed && (
            <Text color="green">
              {"  "}✓ exists on origin{check.kind === "result" && check.local ? " and locally" : ""} — Enter
              will offer to pull it
            </Text>
          )}
          {localOnly && (
            <Text color="yellow">{"  "}! exists locally only — Enter will offer to reuse it</Text>
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
              onSubmit={() => (remoteConfirmed ? doSubmit() : setActiveField("baseBranch"))}
            />
          ) : (
            <Text>{customName || <Text dimColor>(empty)</Text>}</Text>
          )}
        </Box>

        <Box>
          <Text bold={activeField === "baseBranch"} dimColor={remoteConfirmed}>
            Base branch:{" "}
          </Text>
          {remoteConfirmed ? (
            <Text dimColor>(ignored — will track origin/{trimmed})</Text>
          ) : activeField === "baseBranch" ? (
            <TextInput
              value={baseBranch}
              onChange={setBaseBranch}
              onSubmit={doSubmit}
            />
          ) : (
            <Text>{baseBranch || <Text dimColor>(empty)</Text>}</Text>
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
