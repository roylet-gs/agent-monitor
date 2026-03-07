import React, { useState, useEffect } from "react";
import { Box, Text, useInput } from "ink";
import TextInput from "ink-text-input";
import { verifyLinearApiKey } from "../lib/linear.js";
import type { UpdateInfo } from "../hooks/useUpdateCheck.js";
import { DEFAULT_SETTINGS } from "../lib/settings.js";
import type { Settings, Repository } from "../lib/types.js";
import { homedir } from "os";
import { hasStartupScript, openScriptInEditor, removeStartupScript } from "../lib/scripts.js";
import { loadRules, removeRule, clearAllRules, applyRulesToClaudeSettings, removeAmPermissionsFromClaudeSettings } from "../lib/rules.js";
import type { Rule } from "../lib/types.js";

type SettingsField =
  | "ide"
  | "prefix"
  | "baseBranch"
  | "polling"
  | "autoSync"
  | "compactView"
  | "hideMainBranch"
  | "logLevel"
  | "maxLogSize"
  | "ghPrStatus"
  | "ghPolling"
  | "ghRefreshOnManual"
  | "linearEnabled"
  | "linearDesktopApp"
  | "linearApiKey"
  | "linearPolling"
  | "linearRefreshOnManual"
  | "linearAutoNickname"
  | "applyGlobalRules"
  | "manageRules"
  | "removeAllRules"
  | "repos"
  | "checkForUpdates"
  | "resetSettings"
  | "factoryReset";

const FIELDS: SettingsField[] = [
  "ide",
  "prefix",
  "baseBranch",
  "autoSync",
  "compactView",
  "hideMainBranch",
  "polling",
  "logLevel",
  "maxLogSize",
  "ghPrStatus",
  "ghPolling",
  "ghRefreshOnManual",
  "linearEnabled",
  "linearDesktopApp",
  "linearApiKey",
  "linearPolling",
  "linearRefreshOnManual",
  "linearAutoNickname",
  "applyGlobalRules",
  "manageRules",
  "removeAllRules",
  "repos",
  "checkForUpdates",
  "resetSettings",
  "factoryReset",
];

const FIELD_DESCRIPTIONS: Record<SettingsField, string> = {
  ide: "Which editor/IDE to open worktrees in",
  prefix: "Branch name prefix when creating new worktrees (e.g. feature/, fix/)",
  baseBranch: "Default base branch for new worktrees",
  autoSync: "Automatically sync worktree status from git on startup",
  compactView: "Show worktrees in a compact single-line format",
  hideMainBranch: "Hide the main/master branch from the worktree list",
  polling: "How often to check agent status (minimum 0.5s)",
  logLevel: "Verbosity of debug log file at ~/.agent-monitor/debug.log",
  maxLogSize: "Maximum debug log file size before rotation (minimum 1 MB)",
  ghPrStatus: "Show GitHub PR and CI status for each worktree",
  ghPolling: "How often to fetch GitHub PR status (minimum 10s)",
  ghRefreshOnManual: "Include GitHub status when manually refreshing",
  linearEnabled: "Show linked Linear tickets for worktrees",
  linearDesktopApp: "Open Linear links in the desktop app instead of browser",
  linearApiKey: "Personal API key for Linear integration (from Linear Settings > API)",
  linearPolling: "How often to fetch Linear ticket status (minimum 10s)",
  linearRefreshOnManual: "Include Linear tickets when manually refreshing",
  linearAutoNickname: "Auto-set worktree nicknames from Linear ticket titles",
  applyGlobalRules: "Write am rules to ~/.claude/settings.json permissions (persists without TUI)",
  manageRules: "View and remove auto-approval rules",
  removeAllRules: "Remove all auto-approval rules and clean up Claude settings.",
  repos: "Monitored repositories and their startup scripts",
  checkForUpdates: "Check if a newer version of agent-monitor is available",
  resetSettings: "Reset all settings to their default values",
  factoryReset: "Delete all data including repositories, worktrees, and settings",
};

const IDE_OPTIONS: Settings["ide"][] = ["cursor", "vscode", "terminal"];
const LOG_LEVELS: Settings["logLevel"][] = ["debug", "info", "warn", "error"];

interface SettingsPanelProps {
  settings: Settings;
  repositories: Repository[];
  onSave: (settings: Settings) => void;
  onClose: () => void;
  onAddRepo: () => void;
  onRemoveRepo: (repoId: string) => void;
  onSettingsReset: () => void;
  onFactoryReset: () => void;
  onCheckForUpdates: () => Promise<UpdateInfo | null>;
}

export function SettingsPanel({
  settings,
  repositories,
  onSave,
  onClose,
  onAddRepo,
  onRemoveRepo,
  onSettingsReset,
  onFactoryReset,
  onCheckForUpdates,
}: SettingsPanelProps) {
  const [current, setCurrent] = useState({ ...settings });
  const [fieldIndex, setFieldIndex] = useState(0);
  const [repoIndex, setRepoIndex] = useState(0);
  const [editing, setEditing] = useState(false);
  const [editValue, setEditValue] = useState("");
  const [linearVerify, setLinearVerify] = useState<"idle" | "checking" | "ok" | "error">("idle");
  const [linearVerifyMsg, setLinearVerifyMsg] = useState("");
  const [confirming, setConfirming] = useState<"resetSettings" | "factoryReset" | null>(null);
  const [clearRulesMsg, setClearRulesMsg] = useState("");
  const [updateCheckStatus, setUpdateCheckStatus] = useState<"idle" | "checking" | "ok" | "update" | "error">("idle");
  const [updateCheckMsg, setUpdateCheckMsg] = useState("");
  const [showRulesList, setShowRulesList] = useState(false);
  const [rules, setRules] = useState<Rule[]>([]);
  const [ruleIndex, setRuleIndex] = useState(0);

  const verifyLinearKey = (key: string) => {
    if (!key) {
      setLinearVerify("idle");
      setLinearVerifyMsg("");
      return;
    }
    setLinearVerify("checking");
    setLinearVerifyMsg("");
    verifyLinearApiKey(key).then((result) => {
      if (result.ok) {
        setLinearVerify("ok");
        setLinearVerifyMsg(result.name ?? "Connected");
      } else {
        setLinearVerify("error");
        setLinearVerifyMsg(result.error ?? "Invalid key");
      }
    });
  };

  const activeField = FIELDS[fieldIndex];

  const startEditing = () => {
    if (activeField === "prefix") {
      setEditValue(current.defaultBranchPrefix);
      setEditing(true);
    } else if (activeField === "baseBranch") {
      setEditValue(current.defaultBaseBranch);
      setEditing(true);
    } else if (activeField === "polling") {
      setEditValue(String(current.pollingIntervalMs / 1000));
      setEditing(true);
    } else if (activeField === "ghPolling") {
      setEditValue(String(current.ghPollingIntervalMs / 1000));
      setEditing(true);
    } else if (activeField === "linearApiKey") {
      setEditValue(current.linearApiKey);
      setEditing(true);
    } else if (activeField === "linearPolling") {
      setEditValue(String(current.linearPollingIntervalMs / 1000));
      setEditing(true);
    } else if (activeField === "maxLogSize") {
      setEditValue(String(current.maxLogSizeMb));
      setEditing(true);
    }
  };

  const commitEdit = () => {
    if (activeField === "prefix") {
      setCurrent((s) => ({ ...s, defaultBranchPrefix: editValue }));
    } else if (activeField === "baseBranch") {
      setCurrent((s) => ({ ...s, defaultBaseBranch: editValue }));
    } else if (activeField === "polling") {
      const seconds = parseFloat(editValue);
      if (!isNaN(seconds) && seconds >= 0.5) {
        setCurrent((s) => ({ ...s, pollingIntervalMs: Math.round(seconds * 1000) }));
      }
    } else if (activeField === "ghPolling") {
      const seconds = parseFloat(editValue);
      if (!isNaN(seconds) && seconds >= 10) {
        setCurrent((s) => ({ ...s, ghPollingIntervalMs: Math.round(seconds * 1000) }));
      }
    } else if (activeField === "linearApiKey") {
      setCurrent((s) => ({ ...s, linearApiKey: editValue }));
      verifyLinearKey(editValue);
    } else if (activeField === "linearPolling") {
      const seconds = parseFloat(editValue);
      if (!isNaN(seconds) && seconds >= 10) {
        setCurrent((s) => ({ ...s, linearPollingIntervalMs: Math.round(seconds * 1000) }));
      }
    } else if (activeField === "maxLogSize") {
      const mb = parseFloat(editValue);
      if (!isNaN(mb) && mb >= 1) {
        setCurrent((s) => ({ ...s, maxLogSizeMb: Math.round(mb) }));
      }
    }
    setEditing(false);
  };

  const cancelEdit = () => {
    setEditing(false);
  };

  useInput((input, key) => {
    // Rules sub-view input handling
    if (showRulesList) {
      if (key.escape) { setShowRulesList(false); return; }
      if (rules.length === 0) return;
      if (key.upArrow) { setRuleIndex((i) => Math.max(0, i - 1)); return; }
      if (key.downArrow) { setRuleIndex((i) => Math.min(rules.length - 1, i + 1)); return; }
      if ((input === "d" || input === "x") && rules[ruleIndex]) {
        removeRule(rules[ruleIndex].id);
        const updated = loadRules();
        setRules(updated);
        setRuleIndex((i) => updated.length === 0 ? 0 : Math.min(i, updated.length - 1));
        if (current.applyGlobalRulesEnabled) applyRulesToClaudeSettings();
      }
      return;
    }

    // When awaiting confirmation on a danger zone action
    if (confirming) {
      if (input === "y" || input === "Y") {
        if (confirming === "resetSettings") {
          onSettingsReset();
        } else if (confirming === "factoryReset") {
          onFactoryReset();
        }
        setConfirming(null);
      } else if (input === "n" || input === "N" || key.escape) {
        setConfirming(null);
      }
      return;
    }

    // When editing a text field, only handle escape
    if (editing) {
      if (key.escape) {
        cancelEdit();
      }
      return;
    }

    if (key.escape) {
      onSave(current);
      onClose();
      return;
    }

    if (key.tab || key.downArrow) {
      setFieldIndex((i) => Math.min(FIELDS.length - 1, i + 1));
      return;
    }

    if (key.upArrow) {
      setFieldIndex((i) => Math.max(0, i - 1));
      return;
    }

    // Enter on text fields starts editing
    if (
      (activeField === "prefix" ||
        activeField === "baseBranch" ||
        activeField === "polling" ||
        activeField === "ghPolling" ||
        activeField === "linearApiKey" ||
        activeField === "linearPolling" ||
        activeField === "maxLogSize") &&
      key.return
    ) {
      startEditing();
      return;
    }

    if (activeField === "ide" && (key.return || input === " ")) {
      const idx = IDE_OPTIONS.indexOf(current.ide);
      const next = IDE_OPTIONS[(idx + 1) % IDE_OPTIONS.length]!;
      setCurrent((s) => ({ ...s, ide: next }));
      return;
    }

    if (activeField === "autoSync" && (key.return || input === " ")) {
      setCurrent((s) => ({ ...s, autoSyncOnStartup: !s.autoSyncOnStartup }));
      return;
    }

    if (activeField === "compactView" && (key.return || input === " ")) {
      setCurrent((s) => ({ ...s, compactView: !s.compactView }));
      return;
    }

    if (activeField === "hideMainBranch" && (key.return || input === " ")) {
      setCurrent((s) => ({ ...s, hideMainBranch: !s.hideMainBranch }));
      return;
    }

    if (activeField === "ghPrStatus" && (key.return || input === " ")) {
      setCurrent((s) => ({ ...s, ghPrStatus: !s.ghPrStatus }));
      return;
    }

    if (activeField === "ghRefreshOnManual" && (key.return || input === " ")) {
      setCurrent((s) => ({ ...s, ghRefreshOnManual: !s.ghRefreshOnManual }));
      return;
    }

    if (activeField === "linearEnabled" && (key.return || input === " ")) {
      setCurrent((s) => ({ ...s, linearEnabled: !s.linearEnabled }));
      return;
    }

    if (activeField === "linearDesktopApp" && (key.return || input === " ")) {
      setCurrent((s) => ({ ...s, linearUseDesktopApp: !s.linearUseDesktopApp }));
      return;
    }

    if (activeField === "linearRefreshOnManual" && (key.return || input === " ")) {
      setCurrent((s) => ({ ...s, linearRefreshOnManual: !s.linearRefreshOnManual }));
      return;
    }

    if (activeField === "linearAutoNickname" && (key.return || input === " ")) {
      setCurrent((s) => ({ ...s, linearAutoNickname: !s.linearAutoNickname }));
      return;
    }

    if (activeField === "applyGlobalRules" && (key.return || input === " ")) {
      const newEnabled = !current.applyGlobalRulesEnabled;
      setCurrent((s) => ({ ...s, applyGlobalRulesEnabled: newEnabled }));
      if (newEnabled) {
        applyRulesToClaudeSettings();
      } else {
        removeAmPermissionsFromClaudeSettings();
      }
      return;
    }

    if (activeField === "manageRules" && key.return) {
      setRules(loadRules());
      setRuleIndex(0);
      setShowRulesList(true);
      return;
    }

    if (activeField === "removeAllRules" && key.return) {
      const result = clearAllRules();
      if (result.removed === 0) {
        setClearRulesMsg("No rules to remove");
      } else {
        setClearRulesMsg(`Removed ${result.removed} rule${result.removed === 1 ? "" : "s"}`);
        if (current.applyGlobalRulesEnabled) {
          removeAmPermissionsFromClaudeSettings();
        }
      }
      return;
    }


    if (activeField === "logLevel" && (key.return || input === " ")) {
      const idx = LOG_LEVELS.indexOf(current.logLevel);
      const next = LOG_LEVELS[(idx + 1) % LOG_LEVELS.length]!;
      setCurrent((s) => ({ ...s, logLevel: next }));
      return;
    }

    if (activeField === "repos") {
      if (input === "a") {
        onAddRepo();
        return;
      }
      if (input === "r" && repositories[repoIndex]) {
        onRemoveRepo(repositories[repoIndex].id);
        return;
      }
      if (input === "s" && repositories[repoIndex]) {
        openScriptInEditor(repositories[repoIndex].id, current.ide);
        return;
      }
      if (input === "x" && repositories[repoIndex]) {
        removeStartupScript(repositories[repoIndex].id);
        return;
      }
      if (key.leftArrow) {
        setRepoIndex((i) => Math.max(0, i - 1));
        return;
      }
      if (key.rightArrow) {
        setRepoIndex((i) => Math.min(repositories.length - 1, i + 1));
        return;
      }
    }

    if (activeField === "checkForUpdates" && key.return) {
      if (updateCheckStatus === "checking") return;
      setUpdateCheckStatus("checking");
      setUpdateCheckMsg("");
      onCheckForUpdates()
        .then((info) => {
          if (!info) {
            setUpdateCheckStatus("error");
            setUpdateCheckMsg("Check failed");
          } else if (info.updateAvailable) {
            setUpdateCheckStatus("update");
            setUpdateCheckMsg(`v${info.latest} available!`);
          } else {
            setUpdateCheckStatus("ok");
            setUpdateCheckMsg(`Up to date (v${info.current})`);
          }
        })
        .catch(() => {
          setUpdateCheckStatus("error");
          setUpdateCheckMsg("Check failed");
        });
      return;
    }

    if (activeField === "resetSettings" && key.return) {
      setConfirming("resetSettings");
      return;
    }

    if (activeField === "factoryReset" && key.return) {
      setConfirming("factoryReset");
      return;
    }
  });

  const renderSectionHeader = (title: string) => (
    <Box key={`section-${title}`} marginTop={1}>
      <Text dimColor>{"  "}{"─".repeat(2)} </Text>
      <Text bold color="gray">{title}</Text>
      <Text dimColor> {"─".repeat(2)}</Text>
    </Box>
  );

  if (showRulesList) {
    return (
      <Box flexDirection="column" borderStyle="single" paddingX={1}>
        <Text bold color="cyan">Manage Rules</Text>
        <Box marginTop={1} flexDirection="column">
          {rules.length === 0 ? (
            <Text dimColor>No rules. Use `am rule add &lt;tool&gt;` to add one.</Text>
          ) : (
            rules.map((r, i) => (
              <Text key={r.id}>
                {i === ruleIndex ? "▸ " : "  "}
                <Text color={r.decision === "deny" ? "red" : "green"}>{r.decision}</Text>
                {"  "}{r.tool}{r.input_pattern ? `(${r.input_pattern})` : ""}
              </Text>
            ))
          )}
        </Box>
        <Box marginTop={1} borderStyle="single" borderTop borderBottom={false} borderLeft={false} borderRight={false}>
          <Text>
            <Text color="yellow">[↑↓]</Text> Navigate{" "}
            <Text color="yellow">[d]</Text> Remove{" "}
            <Text color="yellow">[Esc]</Text> Back
          </Text>
        </Box>
      </Box>
    );
  }

  return (
    <Box flexDirection="column" borderStyle="single" paddingX={1}>
      <Box>
        <Box flexDirection="column" flexGrow={1}>
          <Text bold color="cyan">
            Settings
          </Text>

          <Box flexDirection="column" marginTop={1}>
            {/* === Worktree Section === */}
        {renderSectionHeader("Worktree")}
        <Box>
          <Text bold={activeField === "ide"}>
            {activeField === "ide" ? "▸" : " "} IDE / Editor:{" "}
          </Text>
          {IDE_OPTIONS.map((opt) => (
            <Text key={opt}>
              {" "}
              <Text color={current.ide === opt ? "cyan" : "gray"}>
                {current.ide === opt ? "●" : "○"} {opt}
              </Text>
            </Text>
          ))}
        </Box>
        <Box>
          <Text bold={activeField === "prefix"}>
            {activeField === "prefix" ? "▸" : " "} Default Branch Prefix:{" "}
          </Text>
          {editing && activeField === "prefix" ? (
            <TextInput
              value={editValue}
              onChange={setEditValue}
              onSubmit={commitEdit}
            />
          ) : (
            <Text>
              {current.defaultBranchPrefix}
              {activeField === "prefix" && <Text dimColor> (Enter to edit)</Text>}
            </Text>
          )}
        </Box>
        <Box>
          <Text bold={activeField === "baseBranch"}>
            {activeField === "baseBranch" ? "▸" : " "} Default Base Branch:{" "}
          </Text>
          {editing && activeField === "baseBranch" ? (
            <TextInput
              value={editValue}
              onChange={setEditValue}
              onSubmit={commitEdit}
            />
          ) : (
            <Text>
              {current.defaultBaseBranch}
              {activeField === "baseBranch" && <Text dimColor> (Enter to edit)</Text>}
            </Text>
          )}
        </Box>
        <Box>
          <Text bold={activeField === "autoSync"}>
            {activeField === "autoSync" ? "▸" : " "} Auto-sync on Startup:{" "}
          </Text>
          <Text color={current.autoSyncOnStartup ? "green" : "gray"}>
            [{current.autoSyncOnStartup ? "✓" : " "}]
          </Text>
        </Box>
        <Box>
          <Text bold={activeField === "compactView"}>
            {activeField === "compactView" ? "▸" : " "} Compact List:{" "}
          </Text>
          <Text color={current.compactView ? "green" : "gray"}>
            [{current.compactView ? "✓" : " "}]
          </Text>
        </Box>
        {/* Hide Main Branch */}
        <Box>
          <Text bold={activeField === "hideMainBranch"}>
            {activeField === "hideMainBranch" ? "▸" : " "} Hide Main Branch:{" "}
          </Text>
          <Text color={current.hideMainBranch ? "green" : "gray"}>
            [{current.hideMainBranch ? "✓" : " "}]
          </Text>
        </Box>
        <Box>
          <Text bold={activeField === "polling"}>
            {activeField === "polling" ? "▸" : " "} Status Poll Interval (s):{" "}
          </Text>
          {editing && activeField === "polling" ? (
            <TextInput
              value={editValue}
              onChange={setEditValue}
              onSubmit={commitEdit}
            />
          ) : (
            <Text>
              {current.pollingIntervalMs / 1000}
              {activeField === "polling" && <Text dimColor> (Enter to edit)</Text>}
            </Text>
          )}
        </Box>
        <Box>
          <Text bold={activeField === "logLevel"}>
            {activeField === "logLevel" ? "▸" : " "} Log Level:{" "}
          </Text>
          {LOG_LEVELS.map((opt) => (
            <Text key={opt}>
              {" "}
              <Text color={current.logLevel === opt ? "cyan" : "gray"}>
                {current.logLevel === opt ? "●" : "○"} {opt}
              </Text>
            </Text>
          ))}
        </Box>
        <Box>
          <Text bold={activeField === "maxLogSize"}>
            {activeField === "maxLogSize" ? "▸" : " "} Max Log Size (MB):{" "}
          </Text>
          {editing && activeField === "maxLogSize" ? (
            <TextInput
              value={editValue}
              onChange={setEditValue}
              onSubmit={commitEdit}
            />
          ) : (
            <Text>
              {current.maxLogSizeMb}
              {activeField === "maxLogSize" && <Text dimColor> (Enter to edit, min 1 MB)</Text>}
            </Text>
          )}
        </Box>

        {/* === GitHub Section === */}
        {renderSectionHeader("GitHub")}
        <Box>
          <Text bold={activeField === "ghPrStatus"}>
            {activeField === "ghPrStatus" ? "▸" : " "} Enabled:{" "}
          </Text>
          <Text color={current.ghPrStatus ? "green" : "gray"}>
            [{current.ghPrStatus ? "✓" : " "}]
          </Text>
        </Box>
        <Box>
          <Text bold={activeField === "ghPolling"}>
            {activeField === "ghPolling" ? "▸" : " "} Poll Interval (s):{" "}
          </Text>
          {editing && activeField === "ghPolling" ? (
            <TextInput
              value={editValue}
              onChange={setEditValue}
              onSubmit={commitEdit}
            />
          ) : (
            <Text>
              {current.ghPollingIntervalMs / 1000}
              {activeField === "ghPolling" && <Text dimColor> (Enter to edit, min 10s)</Text>}
            </Text>
          )}
        </Box>

        <Box>
          <Text bold={activeField === "ghRefreshOnManual"}>
            {activeField === "ghRefreshOnManual" ? "▸" : " "} Include in Refresh:{" "}
          </Text>
          <Text color={current.ghRefreshOnManual ? "green" : "gray"}>
            [{current.ghRefreshOnManual ? "✓" : " "}]
          </Text>
        </Box>

        {/* === Linear Section === */}
        {renderSectionHeader("Linear")}
        <Box>
          <Text bold={activeField === "linearEnabled"}>
            {activeField === "linearEnabled" ? "▸" : " "} Enabled:{" "}
          </Text>
          <Text color={current.linearEnabled ? "green" : "gray"}>
            [{current.linearEnabled ? "✓" : " "}]
          </Text>
        </Box>
        <Box>
          <Text bold={activeField === "linearDesktopApp"}>
            {activeField === "linearDesktopApp" ? "▸" : " "} Use Desktop App:{" "}
          </Text>
          <Text color={current.linearUseDesktopApp ? "green" : "gray"}>
            [{current.linearUseDesktopApp ? "✓" : " "}]
          </Text>
        </Box>
        <Box>
          <Text bold={activeField === "linearApiKey"}>
            {activeField === "linearApiKey" ? "▸" : " "} API Key:{" "}
          </Text>
          {editing && activeField === "linearApiKey" ? (
            <TextInput
              value={editValue}
              onChange={setEditValue}
              onSubmit={commitEdit}
            />
          ) : (
            <Text>
              {current.linearApiKey ? "***" : ""}
              {activeField === "linearApiKey" && !current.linearApiKey && (
                <Text dimColor> (Enter to edit)</Text>
              )}
              {linearVerify === "checking" && <Text color="cyan"> ◌ Verifying...</Text>}
              {linearVerify === "ok" && <Text color="green"> ✓ {linearVerifyMsg}</Text>}
              {linearVerify === "error" && <Text color="red"> ✗ {linearVerifyMsg}</Text>}
            </Text>
          )}
        </Box>
        <Box>
          <Text bold={activeField === "linearPolling"}>
            {activeField === "linearPolling" ? "▸" : " "} Poll Interval (s):{" "}
          </Text>
          {editing && activeField === "linearPolling" ? (
            <TextInput
              value={editValue}
              onChange={setEditValue}
              onSubmit={commitEdit}
            />
          ) : (
            <Text>
              {current.linearPollingIntervalMs / 1000}
              {activeField === "linearPolling" && <Text dimColor> (Enter to edit, min 10s)</Text>}
            </Text>
          )}
        </Box>

        <Box>
          <Text bold={activeField === "linearRefreshOnManual"}>
            {activeField === "linearRefreshOnManual" ? "▸" : " "} Include in Refresh:{" "}
          </Text>
          <Text color={current.linearRefreshOnManual ? "green" : "gray"}>
            [{current.linearRefreshOnManual ? "✓" : " "}]
          </Text>
        </Box>

        <Box>
          <Text bold={activeField === "linearAutoNickname"}>
            {activeField === "linearAutoNickname" ? "▸" : " "} Auto Nickname:{" "}
          </Text>
          <Text color={current.linearAutoNickname ? "green" : "gray"}>
            [{current.linearAutoNickname ? "✓" : " "}]
          </Text>
        </Box>

        {/* === Rules Section === */}
        {renderSectionHeader("Auto-Approval Rules")}
        <Box>
          <Text bold={activeField === "applyGlobalRules"}>
            {activeField === "applyGlobalRules" ? "▸" : " "} Enabled:{" "}
          </Text>
          <Text color={current.applyGlobalRulesEnabled ? "green" : "gray"}>
            [{current.applyGlobalRulesEnabled ? "✓" : " "}]
          </Text>
        </Box>

        <Box>
          <Text bold={activeField === "manageRules"}>
            {activeField === "manageRules" ? "▸" : " "} Manage Rules
          </Text>
          {activeField === "manageRules" && (
            <Text dimColor> (Enter to open)</Text>
          )}
        </Box>

        <Box>
          <Text bold={activeField === "removeAllRules"}>
            {activeField === "removeAllRules" ? "▸" : " "} Remove All Rules
          </Text>
          {activeField === "removeAllRules" && !clearRulesMsg && (
            <Text dimColor> (Enter to remove)</Text>
          )}
          {clearRulesMsg && (
            <Text color="green"> {clearRulesMsg}</Text>
          )}
        </Box>


        {/* === Repositories Section === */}
        {renderSectionHeader("Repositories")}
        <Box flexDirection="column">
          <Text bold={activeField === "repos"}>
            {activeField === "repos" ? "▸" : " "} Repositories
          </Text>
          {repositories.map((repo, i) => (
            <Text key={repo.id}>
              {"    "}
              {activeField === "repos" && i === repoIndex ? (
                <Text color="cyan">▸ </Text>
              ) : (
                "  "
              )}
              {repo.name}
              {hasStartupScript(repo.id) && (
                <Text dimColor> [script]</Text>
              )}
              <Text dimColor>
                {"  "}
                {repo.path.replace(homedir(), "~")}
              </Text>
            </Text>
          ))}
          {activeField === "repos" && (
            <Text dimColor>
              {"    [←→] Select  [a] Add  [r] Remove  [s] Script  [x] Remove script"}
            </Text>
          )}
        </Box>

        {/* === Updates Section === */}
        {renderSectionHeader("Updates")}
        <Box>
          <Text bold={activeField === "checkForUpdates"}>
            {activeField === "checkForUpdates" ? "▸" : " "} Check for Updates
          </Text>
          {updateCheckStatus === "checking" && <Text color="cyan"> ◌ Checking...</Text>}
          {updateCheckStatus === "ok" && <Text color="green"> ✓ {updateCheckMsg}</Text>}
          {updateCheckStatus === "update" && <Text color="green"> ✓ {updateCheckMsg}</Text>}
          {updateCheckStatus === "error" && <Text color="red"> ✗ {updateCheckMsg}</Text>}
          {updateCheckStatus === "idle" && activeField === "checkForUpdates" && (
            <Text dimColor> (Enter to check)</Text>
          )}
        </Box>

        {/* === Danger Zone === */}
        {renderSectionHeader("Danger Zone")}
        <Box>
          <Text bold={activeField === "resetSettings"} color="red">
            {activeField === "resetSettings" ? "▸" : " "} Reset Settings to Defaults
          </Text>
          {activeField === "resetSettings" && confirming === "resetSettings" && (
            <Text color="yellow"> Are you sure? [y/n]</Text>
          )}
          {activeField === "resetSettings" && !confirming && (
            <Text dimColor> (Enter to reset)</Text>
          )}
        </Box>
        <Box>
          <Text bold={activeField === "factoryReset"} color="red">
            {activeField === "factoryReset" ? "▸" : " "} Factory Reset (delete all data)
          </Text>
          {activeField === "factoryReset" && confirming === "factoryReset" && (
            <Text color="yellow"> Are you sure? [y/n]</Text>
          )}
          {activeField === "factoryReset" && !confirming && (
            <Text dimColor> (Enter to reset)</Text>
          )}
        </Box>
          </Box>
        </Box>

        <Box flexDirection="column" borderStyle="round" paddingX={1} width={40} alignSelf="flex-start" marginLeft={1}>
          <Text bold dimColor>Description</Text>
          <Text dimColor wrap="wrap">{FIELD_DESCRIPTIONS[activeField]}</Text>
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
          {editing ? (
            <>
              <Text color="yellow">[Enter]</Text> Save{" "}
              <Text color="yellow">[Esc]</Text> Cancel
            </>
          ) : (
            <>
              <Text color="yellow">[↑↓]</Text> Navigate{" "}
              <Text color="yellow">[Enter/Space]</Text> Toggle/Edit{" "}
              <Text color="yellow">[Esc]</Text> Save & Close
            </>
          )}
        </Text>
      </Box>
    </Box>
  );
}
