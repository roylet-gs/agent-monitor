import React, { useState, useEffect } from "react";
import { Box, Text, useInput } from "ink";
import TextInput from "ink-text-input";
import { verifyLinearApiKey } from "../lib/linear.js";
import { DEFAULT_SETTINGS } from "../lib/settings.js";
import type { Settings, Repository } from "../lib/types.js";
import { homedir } from "os";
import { hasStartupScript, openScriptInEditor, removeStartupScript } from "../lib/scripts.js";

type SettingsField =
  | "ide"
  | "prefix"
  | "polling"
  | "autoHooks"
  | "autoSync"
  | "compactView"
  | "hideMainBranch"
  | "logLevel"
  | "ghPrStatus"
  | "ghPolling"
  | "linearEnabled"
  | "linearApiKey"
  | "linearPolling"
  | "repos"
  | "resetSettings"
  | "factoryReset";

const FIELDS: SettingsField[] = [
  "ide",
  "prefix",
  "autoSync",
  "compactView",
  "hideMainBranch",
  "polling",
  "autoHooks",
  "logLevel",
  "ghPrStatus",
  "ghPolling",
  "linearEnabled",
  "linearApiKey",
  "linearPolling",
  "repos",
  "resetSettings",
  "factoryReset",
];

const IDE_OPTIONS: Settings["ide"][] = ["cursor", "vscode", "terminal"];
const LOG_LEVELS: Settings["logLevel"][] = ["debug", "info", "warn", "error"];

interface SettingsPanelProps {
  settings: Settings;
  repositories: Repository[];
  onSave: (settings: Settings) => void;
  onClose: () => void;
  onAddRepo: () => void;
  onRemoveRepo: (repoId: string) => void;
  onFactoryReset: () => void;
}

export function SettingsPanel({
  settings,
  repositories,
  onSave,
  onClose,
  onAddRepo,
  onRemoveRepo,
  onFactoryReset,
}: SettingsPanelProps) {
  const [current, setCurrent] = useState({ ...settings });
  const [fieldIndex, setFieldIndex] = useState(0);
  const [repoIndex, setRepoIndex] = useState(0);
  const [editing, setEditing] = useState(false);
  const [editValue, setEditValue] = useState("");
  const [linearVerify, setLinearVerify] = useState<"idle" | "checking" | "ok" | "error">("idle");
  const [linearVerifyMsg, setLinearVerifyMsg] = useState("");
  const [confirming, setConfirming] = useState<"resetSettings" | "factoryReset" | null>(null);

  // Verify Linear API key whenever it changes
  useEffect(() => {
    if (!current.linearApiKey) {
      setLinearVerify("idle");
      setLinearVerifyMsg("");
      return;
    }
    let cancelled = false;
    setLinearVerify("checking");
    setLinearVerifyMsg("");
    verifyLinearApiKey(current.linearApiKey).then((result) => {
      if (cancelled) return;
      if (result.ok) {
        setLinearVerify("ok");
        setLinearVerifyMsg(result.name ?? "Connected");
      } else {
        setLinearVerify("error");
        setLinearVerifyMsg(result.error ?? "Invalid key");
      }
    });
    return () => { cancelled = true; };
  }, [current.linearApiKey]);

  const activeField = FIELDS[fieldIndex];

  const startEditing = () => {
    if (activeField === "prefix") {
      setEditValue(current.defaultBranchPrefix);
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
    }
  };

  const commitEdit = () => {
    if (activeField === "prefix") {
      setCurrent((s) => ({ ...s, defaultBranchPrefix: editValue }));
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
    } else if (activeField === "linearPolling") {
      const seconds = parseFloat(editValue);
      if (!isNaN(seconds) && seconds >= 10) {
        setCurrent((s) => ({ ...s, linearPollingIntervalMs: Math.round(seconds * 1000) }));
      }
    }
    setEditing(false);
  };

  const cancelEdit = () => {
    setEditing(false);
  };

  useInput((input, key) => {
    // When awaiting confirmation on a danger zone action
    if (confirming) {
      if (input === "y" || input === "Y") {
        if (confirming === "resetSettings") {
          const defaults = { ...DEFAULT_SETTINGS };
          setCurrent(defaults);
          onSave(defaults);
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
        activeField === "polling" ||
        activeField === "ghPolling" ||
        activeField === "linearApiKey" ||
        activeField === "linearPolling") &&
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

    if (activeField === "autoHooks" && (key.return || input === " ")) {
      setCurrent((s) => ({ ...s, autoInstallHooks: !s.autoInstallHooks }));
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

    if (activeField === "linearEnabled" && (key.return || input === " ")) {
      setCurrent((s) => ({ ...s, linearEnabled: !s.linearEnabled }));
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
    <Box flexDirection="column" key={`section-${title}`}>
      <Text> </Text>
      <Text dimColor bold>
        {"  "}{title}
      </Text>
      <Text dimColor>{"  "}{"─".repeat(title.length + 2)}</Text>
    </Box>
  );

  return (
    <Box flexDirection="column" borderStyle="single" paddingX={1}>
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
        {/* === Agent Section === */}
        {renderSectionHeader("Agent")}
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
          <Text bold={activeField === "autoHooks"}>
            {activeField === "autoHooks" ? "▸" : " "} Auto-install Hooks:{" "}
          </Text>
          <Text color={current.autoInstallHooks ? "green" : "gray"}>
            [{current.autoInstallHooks ? "✓" : " "}]
          </Text>
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
