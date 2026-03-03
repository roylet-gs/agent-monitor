import React, { useState, useEffect, useCallback } from "react";
import { Box, Text, useApp } from "ink";
import { Dashboard } from "./components/Dashboard.js";
import { FolderBrowser } from "./components/FolderBrowser.js";
import { RepoSelector } from "./components/RepoSelector.js";
import { NewWorktreeForm } from "./components/NewWorktreeForm.js";
import { DeleteConfirm } from "./components/DeleteConfirm.js";
import { SettingsPanel } from "./components/SettingsPanel.js";
import { useWorktrees } from "./hooks/useWorktrees.js";
import { useKeyBindings } from "./hooks/useKeyBindings.js";
import {
  getDb,
  addRepository,
  getRepositories,
  touchRepository,
  removeRepository,
  removeWorktree as removeWorktreeDb,
  updateWorktreeCustomName,
} from "./lib/db.js";
import {
  createWorktree as gitCreateWorktree,
  deleteWorktree as gitDeleteWorktree,
  getMainBranch,
  branchExists,
  getRepoName,
} from "./lib/git.js";
import { syncWorktrees } from "./lib/sync.js";
import { installHooks } from "./lib/hooks-installer.js";
import { openInIde } from "./lib/ide-launcher.js";
import { loadSettings, saveSettings } from "./lib/settings.js";
import { log } from "./lib/logger.js";
import type { AppMode, Repository, Settings } from "./lib/types.js";

export function App() {
  const { exit } = useApp();
  const [settings, setSettings] = useState<Settings>(loadSettings);
  const [mode, setMode] = useState<AppMode>("dashboard");
  const [busy, setBusy] = useState<string | null>(null);
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [currentRepo, setCurrentRepo] = useState<Repository | null>(null);
  const [repositories, setRepositories] = useState<Repository[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [escHint, setEscHint] = useState(false);

  // Initialize DB and check for repos
  useEffect(() => {
    getDb();
    const repos = getRepositories();
    setRepositories(repos);
    if (repos.length > 0) {
      setCurrentRepo(repos[0]!);
    } else {
      setMode("folder-browse");
    }
  }, []);

  // Sync worktrees on startup if enabled
  useEffect(() => {
    if (currentRepo && settings.autoSyncOnStartup) {
      setBusy("Syncing worktrees...");
      syncWorktrees(currentRepo.id)
        .then(() => setBusy(null))
        .catch((err) => {
          log("error", "app", `Sync failed: ${err}`);
          setBusy(null);
        });
    }
  }, [currentRepo?.id]);

  const { worktrees, refresh } = useWorktrees(
    currentRepo?.id ?? null,
    settings.pollingIntervalMs
  );

  // Handle open in IDE
  const handleOpen = useCallback(async () => {
    const wt = worktrees[selectedIndex];
    if (!wt) return;

    try {
      if (settings.autoInstallHooks) {
        setBusy("Installing hooks...");
        installHooks(wt.path);
      }
      openInIde(wt.path, settings.ide);
      setBusy(null);
    } catch (err) {
      setError(`${err}`);
      setBusy(null);
    }
  }, [worktrees, selectedIndex, settings]);

  // Handle create worktree
  const handleCreate = useCallback(
    async (branchName: string, customName: string) => {
      if (!currentRepo) return;
      setMode("dashboard");
      setBusy(`Creating worktree ${branchName}...`);

      try {
        const exists = await branchExists(currentRepo.path, branchName);
        if (exists) {
          setError(`Branch ${branchName} already exists`);
          setBusy(null);
          return;
        }

        const mainBranch = await getMainBranch(currentRepo.path);
        const wtPath = await gitCreateWorktree(
          currentRepo.path,
          branchName,
          mainBranch
        );

        if (customName) {
          // After sync, update the custom name
          await syncWorktrees(currentRepo.id);
          await refresh();
          // Find the newly created worktree and set its custom name
          const { getWorktrees } = await import("./lib/db.js");
          const wts = getWorktrees(currentRepo.id);
          const newWt = wts.find((w) => w.branch === branchName);
          if (newWt) {
            updateWorktreeCustomName(newWt.id, customName);
          }
        }

        // Install hooks immediately so Claude picks them up
        if (settings.autoInstallHooks) {
          installHooks(wtPath);
        }

        await syncWorktrees(currentRepo.id);
        await refresh();
        setBusy(null);
        log("info", "app", `Created worktree ${branchName}`);
      } catch (err) {
        setError(`Failed to create worktree: ${err}`);
        setBusy(null);
      }
    },
    [currentRepo, refresh]
  );

  // Handle delete worktree
  const handleDelete = useCallback(async () => {
    const wt = worktrees[selectedIndex];
    if (!wt || !currentRepo) return;
    setMode("dashboard");
    setBusy(`Deleting worktree ${wt.branch}...`);

    try {
      await gitDeleteWorktree(currentRepo.path, wt.path, true);
      removeWorktreeDb(wt.id);
      await refresh();
      setSelectedIndex((i) => Math.max(0, i - 1));
      setBusy(null);
      log("info", "app", `Deleted worktree ${wt.branch}`);
    } catch (err) {
      setError(`Failed to delete worktree: ${err}`);
      setBusy(null);
    }
  }, [worktrees, selectedIndex, currentRepo, refresh]);

  // Handle repo selection from folder browser
  const handleSelectFolder = useCallback(
    async (path: string) => {
      const name = getRepoName(path);
      const repo = addRepository(path, name);
      setRepositories(getRepositories());
      setCurrentRepo(repo);
      setMode("dashboard");

      setBusy("Syncing worktrees...");
      await syncWorktrees(repo.id);
      await refresh();
      setBusy(null);
    },
    [refresh]
  );

  // Handle repo switch
  const handleSwitchRepo = useCallback(
    async (repo: Repository) => {
      touchRepository(repo.id);
      setCurrentRepo(repo);
      setMode("dashboard");
      setSelectedIndex(0);

      setBusy("Syncing worktrees...");
      await syncWorktrees(repo.id);
      await refresh();
      setBusy(null);
    },
    [refresh]
  );

  // Handle settings save
  const handleSaveSettings = useCallback(
    (newSettings: Settings) => {
      setSettings(newSettings);
      saveSettings(newSettings);
    },
    []
  );

  // Handle remove repo from settings
  const handleRemoveRepo = useCallback(
    (repoId: string) => {
      removeRepository(repoId);
      const repos = getRepositories();
      setRepositories(repos);
      if (currentRepo?.id === repoId) {
        if (repos.length > 0) {
          setCurrentRepo(repos[0]!);
        } else {
          setCurrentRepo(null);
          setMode("folder-browse");
        }
      }
    },
    [currentRepo]
  );

  // Key bindings for dashboard mode
  useKeyBindings({
    selectedIndex,
    worktreeCount: worktrees.length,
    mode,
    busy,
    onSelect: setSelectedIndex,
    onEnter: handleOpen,
    onNew: () => setMode("new-worktree"),
    onDelete: () => {
      if (worktrees[selectedIndex]) setMode("delete-confirm");
    },
    onSettings: () => setMode("settings"),
    onRefresh: async () => {
      if (!currentRepo) return;
      setBusy("Syncing worktrees...");
      await syncWorktrees(currentRepo.id);
      await refresh();
      setBusy(null);
    },
    onQuit: () => exit(),
    onEscHint: setEscHint,
  });

  // Clear error after 3 seconds
  useEffect(() => {
    if (error) {
      const timer = setTimeout(() => setError(null), 3000);
      return () => clearTimeout(timer);
    }
  }, [error]);

  return (
    <Box flexDirection="column">
      {error && (
        <Box paddingX={1}>
          <Text color="red">Error: {error}</Text>
        </Box>
      )}

      {mode === "folder-browse" && (
        <FolderBrowser
          onSelect={handleSelectFolder}
          onCancel={() => {
            if (currentRepo) {
              setMode("dashboard");
            } else {
              exit();
            }
          }}
        />
      )}

      {mode === "repo-select" && (
        <RepoSelector
          repositories={repositories}
          onSelect={handleSwitchRepo}
          onCancel={() => setMode("dashboard")}
        />
      )}

      {mode === "new-worktree" && (
        <NewWorktreeForm
          defaultPrefix={settings.defaultBranchPrefix}
          onSubmit={handleCreate}
          onCancel={() => setMode("dashboard")}
        />
      )}

      {mode === "delete-confirm" && worktrees[selectedIndex] && (
        <DeleteConfirm
          worktree={worktrees[selectedIndex]}
          onConfirm={handleDelete}
          onCancel={() => setMode("dashboard")}
        />
      )}

      {mode === "settings" && (
        <SettingsPanel
          settings={settings}
          repositories={repositories}
          onSave={handleSaveSettings}
          onClose={() => setMode("dashboard")}
          onAddRepo={() => setMode("folder-browse")}
          onRemoveRepo={handleRemoveRepo}
        />
      )}

      {mode === "dashboard" && (
        <Dashboard
          repoName={currentRepo?.name ?? "No repository"}
          worktrees={worktrees}
          selectedIndex={selectedIndex}
          busy={busy}
          escHint={escHint}
        />
      )}
    </Box>
  );
}
