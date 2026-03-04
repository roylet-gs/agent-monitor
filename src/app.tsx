import React, { useState, useEffect, useCallback, useRef } from "react";
import { Box, Text, useApp } from "ink";
import { Dashboard } from "./components/Dashboard.js";
import { FolderBrowser } from "./components/FolderBrowser.js";
import { RepoSelector } from "./components/RepoSelector.js";
import { NewWorktreeForm } from "./components/NewWorktreeForm.js";
import { DeleteConfirm, type DeleteOptions } from "./components/DeleteConfirm.js";
import { SettingsPanel } from "./components/SettingsPanel.js";
import { BranchExistsPrompt } from "./components/BranchExistsPrompt.js";
import { CreatingWorktree, type StepInfo } from "./components/CreatingWorktree.js";
import { ProgressSteps } from "./components/ProgressSteps.js";
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
  deleteBranch,
  getMainBranch,
  branchExists,
  getRepoName,
} from "./lib/git.js";
import { syncWorktrees } from "./lib/sync.js";
import { installHooks } from "./lib/hooks-installer.js";
import { openInIde } from "./lib/ide-launcher.js";
import { hasStartupScript, getScriptPath } from "./lib/scripts.js";
import { loadSettings, saveSettings } from "./lib/settings.js";
import { log } from "./lib/logger.js";
import type { AppMode, Repository, Settings } from "./lib/types.js";

interface AppProps {
  onRunScript?: (scriptPath: string, cwd: string) => void;
}

export function App({ onRunScript }: AppProps) {
  const { exit } = useApp();
  const [settings, setSettings] = useState<Settings>(loadSettings);
  const [mode, setMode] = useState<AppMode>("dashboard");
  const [busy, setBusy] = useState<string | null>(null);
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [currentRepo, setCurrentRepo] = useState<Repository | null>(null);
  const [repositories, setRepositories] = useState<Repository[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [escHint, setEscHint] = useState(false);
  const [pendingBranch, setPendingBranch] = useState<{ branch: string; customName: string } | null>(null);
  const [creatingBranch, setCreatingBranch] = useState("");
  const [creationSteps, setCreationSteps] = useState<StepInfo[]>([]);
  const [creationError, setCreationError] = useState<string | null>(null);
  const [deletingBranch, setDeletingBranch] = useState("");
  const [deleteSteps, setDeleteSteps] = useState<StepInfo[]>([]);
  const [deleteError, setDeleteError] = useState<string | null>(null);

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

  const { worktrees, refresh } = useWorktrees({
    repoId: currentRepo?.id ?? null,
    pollingIntervalMs: settings.pollingIntervalMs,
    ghPollingIntervalMs: settings.ghPollingIntervalMs,
    linearPollingIntervalMs: settings.linearPollingIntervalMs,
    ghPrStatus: settings.ghPrStatus,
    linearEnabled: settings.linearEnabled,
    linearApiKey: settings.linearApiKey,
    hideMainBranch: settings.hideMainBranch,
  });

  // Track unseen status changes per worktree
  const seenStatusRef = useRef<Map<string, string>>(new Map());
  const [unseenIds, setUnseenIds] = useState<Set<string>>(new Set());

  useEffect(() => {
    const seen = seenStatusRef.current;
    const newUnseen = new Set<string>();

    for (const wt of worktrees) {
      const currentStatus = wt.agent_status?.status ?? "idle";
      const prevStatus = seen.get(wt.id);

      if (prevStatus === undefined) {
        // First time seeing this worktree — mark as seen
        seen.set(wt.id, currentStatus);
      } else if (prevStatus !== currentStatus) {
        // Status changed — mark unseen
        newUnseen.add(wt.id);
        seen.set(wt.id, currentStatus);
      }
    }

    if (newUnseen.size > 0) {
      setUnseenIds((prev) => {
        const merged = new Set(prev);
        for (const id of newUnseen) merged.add(id);
        return merged;
      });
    }
  }, [worktrees]);

  // Mark selected worktree as seen
  useEffect(() => {
    const selected = worktrees[selectedIndex];
    if (selected && unseenIds.has(selected.id)) {
      setUnseenIds((prev) => {
        const next = new Set(prev);
        next.delete(selected.id);
        return next;
      });
    }
  }, [selectedIndex, worktrees, unseenIds]);

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

      // Check if branch already exists (local or remote)
      const exists = await branchExists(currentRepo.path, branchName);
      if (exists) {
        setPendingBranch({ branch: branchName, customName });
        setMode("branch-exists");
        return;
      }

      // Check if worktree path already exists on disk (stale from previous removal)
      const { existsSync } = await import("fs");
      const { join } = await import("path");
      const wtDir = join(currentRepo.path, ".worktrees", branchName.replace(/\//g, "-"));
      if (existsSync(wtDir)) {
        setPendingBranch({ branch: branchName, customName });
        setMode("branch-exists");
        return;
      }

      await doCreateWorktree(branchName, customName, false);
    },
    [currentRepo]
  );

  // Helper to update a single step in the creation steps array
  const updateStep = useCallback((index: number, status: StepInfo["status"]) => {
    setCreationSteps((prev) =>
      prev.map((s, i) => (i === index ? { ...s, status } : s))
    );
  }, []);

  // Helper to update a single step in the delete steps array
  const updateDeleteStep = useCallback((index: number, status: StepInfo["status"]) => {
    setDeleteSteps((prev) =>
      prev.map((s, i) => (i === index ? { ...s, status } : s))
    );
  }, []);

  // Actually create the worktree (called directly or after branch-exists confirmation)
  const doCreateWorktree = useCallback(
    async (branchName: string, customName: string, reuse: boolean) => {
      if (!currentRepo) return;

      const hasScript = hasStartupScript(currentRepo.id);

      // Build steps list
      const steps: StepInfo[] = [
        { label: "Creating git worktree", status: "active" },
        { label: "Syncing database", status: "pending" },
        ...(settings.autoInstallHooks
          ? [{ label: "Installing hooks", status: "pending" as const }]
          : []),
        ...(hasScript
          ? [{ label: "Running startup script", status: "pending" as const }]
          : []),
      ];

      setCreatingBranch(branchName);
      setCreationSteps(steps);
      setCreationError(null);
      setMode("creating-worktree");

      let stepIdx = 0;

      try {
        // Step: Create git worktree
        const mainBranch = await getMainBranch(currentRepo.path);
        let wtPath: string;
        try {
          wtPath = await gitCreateWorktree(
            currentRepo.path,
            branchName,
            mainBranch,
            reuse
          );
        } catch (gitErr) {
          // If branch/ref already exists, offer to reuse
          const errMsg = String(gitErr);
          if (errMsg.includes("already exists") || errMsg.includes("already checked out")) {
            updateStep(stepIdx, "error");
            setCreationError(null);
            setMode("dashboard");
            setPendingBranch({ branch: branchName, customName });
            setMode("branch-exists");
            return;
          }
          throw gitErr;
        }
        updateStep(stepIdx, "done");
        stepIdx++;

        // Step: Sync database
        updateStep(stepIdx, "active");
        if (customName) {
          await syncWorktrees(currentRepo.id);
          await refresh();
          const { getWorktrees } = await import("./lib/db.js");
          const wts = getWorktrees(currentRepo.id);
          const newWt = wts.find((w) => w.branch === branchName);
          if (newWt) {
            updateWorktreeCustomName(newWt.id, customName);
          }
        }
        await syncWorktrees(currentRepo.id);
        await refresh();
        updateStep(stepIdx, "done");
        stepIdx++;

        // Step: Install hooks
        if (settings.autoInstallHooks) {
          updateStep(stepIdx, "active");
          installHooks(wtPath);
          updateStep(stepIdx, "done");
          stepIdx++;
        }

        log("info", "app", `Created worktree ${branchName}`);

        // Step: Run startup script — hand off to CLI runner
        if (hasScript && onRunScript) {
          updateStep(stepIdx, "active");
          // Small delay so the user sees the "Running startup script" step
          await new Promise((r) => setTimeout(r, 300));
          onRunScript(getScriptPath(currentRepo.id), wtPath);
          exit();
          return;
        }

        // All done — go to dashboard
        setMode("dashboard");
      } catch (err) {
        updateStep(stepIdx, "error");
        setCreationError(`${err}`);
        // Stay on creating-worktree screen showing the error
        // After 3s, go back to dashboard
        setTimeout(() => {
          setMode("dashboard");
          setError(`Failed to create worktree: ${err}`);
        }, 3000);
      }
    },
    [currentRepo, refresh, settings, onRunScript, exit, updateStep]
  );

  // Handle delete worktree
  const handleDelete = useCallback(async (options: DeleteOptions) => {
    const wt = worktrees[selectedIndex];
    if (!wt || !currentRepo) return;

    // Build steps list
    const steps: StepInfo[] = [
      { label: "Removing worktree", status: "active" },
      ...(options.deleteLocalBranch
        ? [{ label: `Deleting local branch ${wt.branch}`, status: "pending" as const }]
        : []),
      { label: "Syncing database", status: "pending" },
    ];

    setDeletingBranch(wt.custom_name ?? wt.branch);
    setDeleteSteps(steps);
    setDeleteError(null);
    setMode("deleting-worktree");

    let stepIdx = 0;

    try {
      // Step: Remove worktree
      await gitDeleteWorktree(currentRepo.path, wt.path, true);
      removeWorktreeDb(wt.id);
      updateDeleteStep(stepIdx, "done");
      stepIdx++;

      // Step: Delete local branch
      if (options.deleteLocalBranch) {
        updateDeleteStep(stepIdx, "active");
        try {
          await deleteBranch(currentRepo.path, wt.branch, true);
          updateDeleteStep(stepIdx, "done");
        } catch (err) {
          updateDeleteStep(stepIdx, "error");
          log("warn", "app", `Failed to delete local branch: ${err}`);
        }
        stepIdx++;
      }

      // Step: Sync database
      updateDeleteStep(stepIdx, "active");
      await refresh();
      updateDeleteStep(stepIdx, "done");

      setSelectedIndex((i) => Math.max(0, i - 1));
      log("info", "app", `Deleted worktree ${wt.branch}`);

      // Brief pause so user sees the completed steps
      await new Promise((r) => setTimeout(r, 500));
      setMode("dashboard");
    } catch (err) {
      updateDeleteStep(stepIdx, "error");
      setDeleteError(`${err}`);
      setTimeout(() => {
        setMode("dashboard");
        setError(`Failed to delete worktree: ${err}`);
      }, 3000);
    }
  }, [worktrees, selectedIndex, currentRepo, refresh, updateDeleteStep]);

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
    onOpenPr: () => {
      const wt = worktrees[selectedIndex];
      if (wt?.pr_info?.url) {
        import("open").then((mod) => mod.default(wt.pr_info!.url)).catch(() => {});
      }
    },
    onOpenLinear: () => {
      const wt = worktrees[selectedIndex];
      if (wt?.linear_info?.url) {
        import("open").then((mod) => mod.default(wt.linear_info!.url)).catch(() => {});
      }
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

      {mode === "branch-exists" && pendingBranch && (
        <BranchExistsPrompt
          branchName={pendingBranch.branch}
          onReuse={() => {
            doCreateWorktree(pendingBranch.branch, pendingBranch.customName, true);
            setPendingBranch(null);
          }}
          onDeleteAndRecreate={async () => {
            if (!currentRepo) return;
            const branch = pendingBranch.branch;
            const customName = pendingBranch.customName;
            setPendingBranch(null);
            try {
              await deleteBranch(currentRepo.path, branch, true);
            } catch {
              // Branch may only exist on remote, ignore local delete failure
            }
            await doCreateWorktree(branch, customName, false);
          }}
          onCancel={() => {
            setPendingBranch(null);
            setMode("new-worktree");
          }}
        />
      )}

      {mode === "creating-worktree" && (
        <CreatingWorktree
          branchName={creatingBranch}
          steps={creationSteps}
          error={creationError}
        />
      )}

      {mode === "deleting-worktree" && (
        <ProgressSteps
          title="Delete Worktree"
          subtitle={`Deleting ${deletingBranch}...`}
          steps={deleteSteps}
          error={deleteError}
        />
      )}

      {mode === "delete-confirm" && worktrees[selectedIndex] && currentRepo && (
        <DeleteConfirm
          worktree={worktrees[selectedIndex]}
          repoPath={currentRepo.path}
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
          unseenIds={unseenIds}
          compactView={settings.compactView}
        />
      )}
    </Box>
  );
}
