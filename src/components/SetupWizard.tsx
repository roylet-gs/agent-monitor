import React, { useState, useEffect } from "react";
import { Box, Text, useInput } from "ink";
import TextInput from "ink-text-input";
import { FolderBrowser } from "./FolderBrowser.js";
import { isGhAvailable } from "../lib/github.js";
import { verifyLinearApiKey } from "../lib/linear.js";
import { installGlobalHooks, isGlobalHooksInstalled } from "../lib/hooks-installer.js";
import type { Settings } from "../lib/types.js";

type WizardStep = "welcome" | "ide" | "branches" | "github" | "linear" | "repo" | "hooks" | "done";

const STEPS: WizardStep[] = ["welcome", "ide", "branches", "github", "linear", "repo", "hooks", "done"];
const TOTAL_STEPS = STEPS.length - 2; // exclude welcome and done from numbering

const IDE_OPTIONS: Settings["ide"][] = ["cursor", "vscode", "terminal"];
const IDE_LABELS: Record<Settings["ide"], string> = {
  cursor: "Cursor",
  vscode: "VS Code",
  terminal: "Terminal (vim, etc.)",
};

interface SetupWizardProps {
  initialSettings: Settings;
  onComplete: (settings: Settings, repoPath: string | null) => void;
  onSkip: () => void;
}

export function SetupWizard({ initialSettings, onComplete, onSkip }: SetupWizardProps) {
  const [step, setStep] = useState<WizardStep>("welcome");
  const [draft, setDraft] = useState<Settings>({ ...initialSettings });
  const [selectedRepoPath, setSelectedRepoPath] = useState<string | null>(null);

  // Per-step UI state
  const [ideIndex, setIdeIndex] = useState(() => Math.max(0, IDE_OPTIONS.indexOf(initialSettings.ide)));
  const [branchField, setBranchField] = useState<"prefix" | "base">("prefix");
  const [editingBranch, setEditingBranch] = useState(false);
  const [branchEditValue, setBranchEditValue] = useState("");
  const [ghAvailable, setGhAvailable] = useState<boolean | null>(null);
  const [linearEnabled, setLinearEnabled] = useState(initialSettings.linearEnabled);
  const [linearKeyValue, setLinearKeyValue] = useState(initialSettings.linearApiKey);
  const [editingLinearKey, setEditingLinearKey] = useState(false);
  const [linearVerify, setLinearVerify] = useState<"idle" | "checking" | "ok" | "error">("idle");
  const [linearVerifyMsg, setLinearVerifyMsg] = useState("");
  const [hooksInstalled, setHooksInstalled] = useState(() => isGlobalHooksInstalled());
  const [hooksJustInstalled, setHooksJustInstalled] = useState(false);

  // Auto-detect gh on github step
  useEffect(() => {
    if (step === "github" && ghAvailable === null) {
      setGhAvailable(isGhAvailable());
    }
  }, [step]);

  const stepNumber = (): number => {
    const idx = STEPS.indexOf(step);
    // welcome=0, ide=1, branches=2, ..., hooks=6, done=7
    return idx; // 1-based for display: idx itself since welcome is 0
  };

  const advance = () => {
    const idx = STEPS.indexOf(step);
    if (idx < STEPS.length - 1) {
      setStep(STEPS[idx + 1]);
    }
  };

  const renderProgress = () => {
    const num = stepNumber();
    if (num === 0 || step === "done") return null;
    const filled = Math.round((num / TOTAL_STEPS) * 7);
    const bar = "\u2588".repeat(filled) + "\u2591".repeat(7 - filled);
    return (
      <Box marginBottom={1}>
        <Text dimColor>Step {num}/{TOTAL_STEPS}  {bar}</Text>
      </Box>
    );
  };

  useInput((input, key) => {
    // Global: Esc skips wizard from any step
    if (key.escape) {
      onSkip();
      return;
    }

    if (step === "welcome") {
      if (key.return) advance();
      return;
    }

    if (step === "ide") {
      if (key.upArrow) {
        setIdeIndex((i) => Math.max(0, i - 1));
      } else if (key.downArrow) {
        setIdeIndex((i) => Math.min(IDE_OPTIONS.length - 1, i + 1));
      } else if (key.return || input === " ") {
        setDraft((d) => ({ ...d, ide: IDE_OPTIONS[ideIndex] }));
        advance();
      }
      return;
    }

    if (step === "branches") {
      if (editingBranch) {
        // TextInput handles typing; we only handle escape
        return;
      }
      if (key.upArrow || key.downArrow) {
        setBranchField((f) => (f === "prefix" ? "base" : "prefix"));
      } else if (key.return) {
        setBranchEditValue(branchField === "prefix" ? draft.defaultBranchPrefix : draft.defaultBaseBranch);
        setEditingBranch(true);
      } else if (key.tab) {
        advance();
      }
      return;
    }

    if (step === "github") {
      if (input === " " || key.return) {
        if (ghAvailable) {
          setDraft((d) => ({ ...d, ghPrStatus: !d.ghPrStatus }));
        }
      } else if (key.tab) {
        advance();
      }
      return;
    }

    if (step === "linear") {
      if (editingLinearKey) return;
      if (input === " " && !editingLinearKey) {
        const next = !linearEnabled;
        setLinearEnabled(next);
        setDraft((d) => ({ ...d, linearEnabled: next }));
        if (next && !linearKeyValue) {
          setEditingLinearKey(true);
          setLinearKeyValue("");
        }
      } else if (key.return && linearEnabled && !editingLinearKey) {
        setEditingLinearKey(true);
        setLinearKeyValue(draft.linearApiKey);
      } else if (key.tab) {
        advance();
      }
      return;
    }

    // step === "repo" is handled by FolderBrowser (useInput is inactive)

    if (step === "hooks") {
      if (key.return) {
        installGlobalHooks();
        setHooksInstalled(true);
        setHooksJustInstalled(true);
        setTimeout(() => advance(), 300);
      } else if (input === "s" || key.tab) {
        advance();
      }
      return;
    }

    if (step === "done") {
      if (key.return) {
        onComplete(draft, selectedRepoPath);
      }
      return;
    }
  }, { isActive: step !== "repo" });

  const commitBranchEdit = () => {
    if (branchField === "prefix") {
      setDraft((d) => ({ ...d, defaultBranchPrefix: branchEditValue }));
    } else {
      setDraft((d) => ({ ...d, defaultBaseBranch: branchEditValue }));
    }
    setEditingBranch(false);
  };

  const commitLinearKey = () => {
    setDraft((d) => ({ ...d, linearApiKey: linearKeyValue }));
    setEditingLinearKey(false);
    if (linearKeyValue) {
      setLinearVerify("checking");
      setLinearVerifyMsg("");
      verifyLinearApiKey(linearKeyValue).then((result) => {
        if (result.ok) {
          setLinearVerify("ok");
          setLinearVerifyMsg(result.name ?? "Connected");
        } else {
          setLinearVerify("error");
          setLinearVerifyMsg(result.error ?? "Invalid key");
        }
      });
    } else {
      setLinearVerify("idle");
    }
  };

  return (
    <Box flexDirection="column" paddingX={1} paddingY={1}>
      {renderProgress()}

      {step === "welcome" && (
        <Box flexDirection="column">
          <Text bold color="cyan">Welcome to Agent Monitor</Text>
          <Box marginTop={1}>
            <Text>
              Agent Monitor is a TUI dashboard for managing git worktrees{"\n"}
              and monitoring Claude Code agent sessions.{"\n"}
              {"\n"}
              This wizard will help you configure:{"\n"}
              {"  "}- Your preferred IDE{"\n"}
              {"  "}- Branch naming defaults{"\n"}
              {"  "}- GitHub and Linear integrations{"\n"}
              {"  "}- Your first repository{"\n"}
              {"  "}- Claude Code hooks for real-time status
            </Text>
          </Box>
          <Box marginTop={1}>
            <Text dimColor>[Enter] Start setup  [Esc] Skip (use defaults)</Text>
          </Box>
        </Box>
      )}

      {step === "ide" && (
        <Box flexDirection="column">
          <Text bold>IDE / Editor</Text>
          <Text dimColor>Which editor should open when you select a worktree?</Text>
          <Box flexDirection="column" marginTop={1}>
            {IDE_OPTIONS.map((opt, i) => (
              <Box key={opt}>
                <Text color={i === ideIndex ? "cyan" : undefined}>
                  {i === ideIndex ? ">" : " "} {i === ideIndex ? "\u25CF" : "\u25CB"} {IDE_LABELS[opt]}
                </Text>
              </Box>
            ))}
          </Box>
          <Box marginTop={1}>
            <Text dimColor>[Up/Down] Select  [Enter] Confirm</Text>
          </Box>
        </Box>
      )}

      {step === "branches" && (
        <Box flexDirection="column">
          <Text bold>Branch Defaults</Text>
          <Text dimColor>Configure defaults for new worktree branches.</Text>
          <Box flexDirection="column" marginTop={1}>
            <Box>
              <Text bold={branchField === "prefix"}>
                {branchField === "prefix" ? "\u25B8" : " "} Branch Prefix:{" "}
              </Text>
              {editingBranch && branchField === "prefix" ? (
                <TextInput
                  value={branchEditValue}
                  onChange={setBranchEditValue}
                  onSubmit={commitBranchEdit}
                />
              ) : (
                <Text>
                  {draft.defaultBranchPrefix || <Text dimColor>(none)</Text>}
                  {branchField === "prefix" && !editingBranch && <Text dimColor> [Enter to edit]</Text>}
                </Text>
              )}
            </Box>
            <Box>
              <Text bold={branchField === "base"}>
                {branchField === "base" ? "\u25B8" : " "} Base Branch:{" "}
              </Text>
              {editingBranch && branchField === "base" ? (
                <TextInput
                  value={branchEditValue}
                  onChange={setBranchEditValue}
                  onSubmit={commitBranchEdit}
                />
              ) : (
                <Text>
                  {draft.defaultBaseBranch}
                  {branchField === "base" && !editingBranch && <Text dimColor> [Enter to edit]</Text>}
                </Text>
              )}
            </Box>
          </Box>
          <Box marginTop={1}>
            <Text dimColor>[Up/Down] Select field  [Enter] Edit  [Tab] Continue</Text>
          </Box>
        </Box>
      )}

      {step === "github" && (
        <Box flexDirection="column">
          <Text bold>GitHub Integration</Text>
          <Text dimColor>Show PR and CI status for each worktree branch.</Text>
          <Box flexDirection="column" marginTop={1}>
            <Box>
              <Text>gh CLI: </Text>
              {ghAvailable === null ? (
                <Text color="cyan">Detecting...</Text>
              ) : ghAvailable ? (
                <Text color="green">Installed</Text>
              ) : (
                <Text color="yellow">Not found</Text>
              )}
            </Box>
            {!ghAvailable && ghAvailable !== null && (
              <Text dimColor>Install the GitHub CLI (gh) to enable PR status.</Text>
            )}
            <Box marginTop={1}>
              <Text>
                {"\u25B8"} PR Status:{" "}
              </Text>
              <Text color={draft.ghPrStatus ? "green" : "gray"}>
                [{draft.ghPrStatus ? "\u2713" : " "}] {draft.ghPrStatus ? "Enabled" : "Disabled"}
              </Text>
            </Box>
          </Box>
          <Box marginTop={1}>
            <Text dimColor>[Space/Enter] Toggle  [Tab] Continue</Text>
          </Box>
        </Box>
      )}

      {step === "linear" && (
        <Box flexDirection="column">
          <Text bold>Linear Integration</Text>
          <Text dimColor>Show linked Linear tickets for worktree branches.</Text>
          <Box flexDirection="column" marginTop={1}>
            <Box>
              <Text>
                {"\u25B8"} Enabled:{" "}
              </Text>
              <Text color={linearEnabled ? "green" : "gray"}>
                [{linearEnabled ? "\u2713" : " "}]
              </Text>
            </Box>
            {linearEnabled && (
              <Box marginTop={1} flexDirection="column">
                <Text dimColor>
                  Get your API key from Linear: Settings {">"} Account {">"} Security {">"} Personal API keys{"\n"}
                  https://linear.app/settings/account/security
                </Text>
                <Box marginTop={1}>
                <Text>  API Key: </Text>
                {editingLinearKey ? (
                  <TextInput
                    value={linearKeyValue}
                    onChange={setLinearKeyValue}
                    onSubmit={commitLinearKey}
                  />
                ) : (
                  <Text>
                    {draft.linearApiKey ? "***" : <Text dimColor>(not set)</Text>}
                    {!editingLinearKey && <Text dimColor> [Enter to edit]</Text>}
                    {linearVerify === "checking" && <Text color="cyan"> Verifying...</Text>}
                    {linearVerify === "ok" && <Text color="green"> {"\u2713"} {linearVerifyMsg}</Text>}
                    {linearVerify === "error" && <Text color="red"> {"\u2717"} {linearVerifyMsg}</Text>}
                  </Text>
                )}
              </Box>
              </Box>
            )}
          </Box>
          <Box marginTop={1}>
            <Text dimColor>[Space] Toggle  [Enter] Edit key  [Tab] Continue</Text>
          </Box>
        </Box>
      )}

      {step === "repo" && (
        <Box flexDirection="column">
          <Box marginBottom={1}>
            <Text bold>Select a Repository</Text>
            <Text dimColor>  Choose a git repository to monitor.</Text>
          </Box>
          <FolderBrowser
            onSelect={(path) => {
              setSelectedRepoPath(path);
              advance();
            }}
            onCancel={() => {
              setSelectedRepoPath(null);
              advance();
            }}
          />
        </Box>
      )}

      {step === "hooks" && (
        <Box flexDirection="column">
          <Text bold>Claude Code Hooks</Text>
          <Text dimColor>
            Hooks let Agent Monitor receive real-time status updates{"\n"}
            from Claude Code sessions via hook events in ~/.claude/settings.json.
          </Text>
          <Box marginTop={1}>
            <Text>Status: </Text>
            {hooksInstalled ? (
              <Text color="green">{"\u2713"} Installed{hooksJustInstalled ? " (just now)" : ""}</Text>
            ) : (
              <Text color="yellow">Not installed</Text>
            )}
          </Box>
          <Box marginTop={1}>
            {hooksInstalled ? (
              <Text dimColor>[Enter] Continue  [s] Skip</Text>
            ) : (
              <Text dimColor>[Enter] Install hooks  [s] Skip</Text>
            )}
          </Box>
        </Box>
      )}

      {step === "done" && (
        <Box flexDirection="column">
          <Text bold color="green">Setup Complete</Text>
          <Box flexDirection="column" marginTop={1} paddingX={1}>
            <Text>  IDE:            <Text color="cyan">{draft.ide}</Text></Text>
            <Text>  Branch Prefix:  <Text color="cyan">{draft.defaultBranchPrefix || "(none)"}</Text></Text>
            <Text>  Base Branch:    <Text color="cyan">{draft.defaultBaseBranch}</Text></Text>
            <Text>  GitHub PR:      <Text color={draft.ghPrStatus ? "green" : "gray"}>{draft.ghPrStatus ? "Enabled" : "Disabled"}</Text></Text>
            <Text>  Linear:         <Text color={draft.linearEnabled ? "green" : "gray"}>{draft.linearEnabled ? "Enabled" : "Disabled"}</Text></Text>
            <Text>  Repository:     <Text color="cyan">{selectedRepoPath ?? "(none selected)"}</Text></Text>
            <Text>  Hooks:          <Text color={hooksInstalled ? "green" : "yellow"}>{hooksInstalled ? "Installed" : "Skipped"}</Text></Text>
          </Box>
          <Box marginTop={1}>
            <Text dimColor>You can change any of these later in Settings (press 's' on the dashboard).</Text>
          </Box>
          <Box marginTop={1}>
            <Text dimColor>[Enter] Launch dashboard</Text>
          </Box>
        </Box>
      )}
    </Box>
  );
}
