import React, { useState } from "react";
import { Box, Text, useInput } from "ink";
import TextInput from "ink-text-input";
import type { Settings, Repository } from "../lib/types.js";
import { homedir } from "os";
import { hasStartupScript, openScriptInEditor, removeStartupScript } from "../lib/scripts.js";

type SettingsField =
  | "ide"
  | "prefix"
  | "polling"
  | "autoHooks"
  | "autoSync"
  | "logLevel"
  | "repos";

const FIELDS: SettingsField[] = [
  "ide",
  "prefix",
  "polling",
  "autoHooks",
  "autoSync",
  "logLevel",
  "repos",
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
}

export function SettingsPanel({
  settings,
  repositories,
  onSave,
  onClose,
  onAddRepo,
  onRemoveRepo,
}: SettingsPanelProps) {
  const [current, setCurrent] = useState({ ...settings });
  const [fieldIndex, setFieldIndex] = useState(0);
  const [repoIndex, setRepoIndex] = useState(0);
  const [editing, setEditing] = useState(false);
  const [editValue, setEditValue] = useState("");

  const activeField = FIELDS[fieldIndex];

  const startEditing = () => {
    if (activeField === "prefix") {
      setEditValue(current.defaultBranchPrefix);
      setEditing(true);
    } else if (activeField === "polling") {
      setEditValue(String(current.pollingIntervalMs / 1000));
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
    }
    setEditing(false);
  };

  const cancelEdit = () => {
    setEditing(false);
  };

  useInput((input, key) => {
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
    if ((activeField === "prefix" || activeField === "polling") && key.return) {
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
  });

  return (
    <Box flexDirection="column" borderStyle="single" paddingX={1}>
      <Text bold color="cyan">
        Settings
      </Text>

      <Box flexDirection="column" marginTop={1} gap={1}>
        {/* IDE */}
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

        {/* Default Branch Prefix */}
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

        {/* Polling Interval */}
        <Box>
          <Text bold={activeField === "polling"}>
            {activeField === "polling" ? "▸" : " "} Polling Interval (s):{" "}
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

        {/* Auto Install Hooks */}
        <Box>
          <Text bold={activeField === "autoHooks"}>
            {activeField === "autoHooks" ? "▸" : " "} Auto-install Claude Hooks:{" "}
          </Text>
          <Text color={current.autoInstallHooks ? "green" : "gray"}>
            [{current.autoInstallHooks ? "✓" : " "}]
          </Text>
        </Box>

        {/* Auto Sync */}
        <Box>
          <Text bold={activeField === "autoSync"}>
            {activeField === "autoSync" ? "▸" : " "} Auto-sync on Startup:{" "}
          </Text>
          <Text color={current.autoSyncOnStartup ? "green" : "gray"}>
            [{current.autoSyncOnStartup ? "✓" : " "}]
          </Text>
        </Box>

        {/* Log Level */}
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

        {/* Repositories */}
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
              {"    [a] Add  [r] Remove  [s] Script  [x] Remove script"}
            </Text>
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
              <Text color="yellow">[Tab/↑↓]</Text> Navigate{" "}
              <Text color="yellow">[Enter/Space]</Text> Toggle/Edit{" "}
              <Text color="yellow">[Esc]</Text> Save & Close
            </>
          )}
        </Text>
      </Box>
    </Box>
  );
}
