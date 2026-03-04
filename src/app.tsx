import React, { useState, useEffect, useCallback, useRef, useMemo } from "react";
import { Box, Text, useApp, useStdout } from "ink";
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
  resetAll,
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
import { loadSettings, saveSettings, DEFAULT_SETTINGS } from "./lib/settings.js";
import { log } from "./lib/logger.js";
import type { AppMode, Repository, Settings } from "./lib/types.js";

interface AppProps {
  onRunScript?: (scriptPath: string, cwd: string) => void;
}

export function App({ onRunScript }: AppProps) {
  const { exit } = useApp();
  const { stdout } = useStdout();
  const [settings, setSettings] = useState<Settings>(loadSettings);
  const [mode, setMode] = useState<AppMode>("dashboard");
  const [busy, setBusy] = useState<string | null>(null);
  const [selectedIndex, setSelectedIndex] = useState(0);
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
  // For create-worktree flow: repo picked from RepoSelector
  const [createTargetRepo, setCreateTargetRepo] = useState<Repository | null>(null);

  // Initialize DB and check for repos
  useEffect(() => {
    getDb();
    const repos = getRepositories();
    setRepositories(repos);
    if (repos.length === 0) {
      setMode("folder-browse");
    }
  }, []);

  // Sync all repos on startup if enabled
  useEffect(() => {
    if (repositories.length > 0 && settings.autoSyncOnStartup) {
      setBusy("Syncing worktrees...");
      Promise.all(repositories.map((repo) => syncWorktrees(repo.id)))
        .then(() => refreshRef.current())
        .then(() => setBusy(null))
        .catch((err) => {
          log("error", "app", `Sync failed: ${err}`);
          setBusy(null);
        });
    }
  }, [repositories.length > 0 && settings.autoSyncOnStartup]);

  const { groups, flatWorktrees, refresh } = useWorktrees({
    repositories,
    pollingIntervalMs: settings.pollingIntervalMs,
    ghPollingIntervalMs: settings.ghPollingIntervalMs,
    linearPollingIntervalMs: settings.linearPollingIntervalMs,
    ghPrStatus: settings.ghPrStatus,
    linearEnabled: settings.linearEnabled,
    linearApiKey: settings.linearApiKey,
    hideMainBranch: settings.hideMainBranch,
  });

  // Keep a ref to always call the latest refresh (avoids stale closures in async handlers)
  const refreshRef = useRef(refresh);
  useEffect(() => { refreshRef.current = refresh; }, [refresh]);

  // Derive the active repo from the currently selected worktree
  const activeRepo = useMemo((): Repository | null => {
    const selected = flatWorktrees[selectedIndex];
    if (!selected) return repositories[0] ?? null;
    return repositories.find((r) => r.id === selected.repo_id) ?? repositories[0] ?? null;
  }, [flatWorktrees, selectedIndex, repositories]);

  // Track unseen status changes per worktree
  const seenStatusRef = useRef<Map<string, string>>(new Map());
  const [unseenIds, setUnseenIds] = useState<Set<string>>(new Set());

  useEffect(() => {
    const seen = seenStatusRef.current;
    const newUnseen = new Set<string>();

    for (const wt of flatWorktrees) {
      const currentStatus = wt.agent_status?.status ?? "idle";
      const prevStatus = seen.get(wt.id);

      if (prevStatus === undefined) {
        seen.set(wt.id, currentStatus);
      } else if (prevStatus !== currentStatus) {
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
  }, [flatWorktrees]);

  // Mark selected worktree as seen
  useEffect(() => {
    const selected = flatWorktrees[selectedIndex];
    if (selected && unseenIds.has(selected.id)) {
      setUnseenIds((prev) => {
        const next = new Set(prev);
        next.delete(selected.id);
        return next;
      });
    }
  }, [selectedIndex, flatWorktrees, unseenIds]);

  // Handle open in IDE
  const handleOpen = useCallback(async () => {
    const wt = flatWorktrees[selectedIndex];
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
  }, [flatWorktrees, selectedIndex, settings]);

  // Handle create worktree
  const handleCreate = useCallback(
    async (branchName: string, customName: string) => {
      const repo = createTargetRepo ?? activeRepo;
      if (!repo) return;

      const exists = await branchExists(repo.path, branchName);
      if (exists) {
        setPendingBranch({ branch: branchName, customName });
        setMode("branch-exists");
        return;
      }

      const { existsSync } = await import("fs");
      const { join } = await import("path");
      const wtDir = join(repo.path, ".worktrees", branchName.replace(/\//g, "-"));
      if (existsSync(wtDir)) {
        setPendingBranch({ branch: branchName, customName });
        setMode("branch-exists");
        return;
      }

      await doCreateWorktree(branchName, customName, false, repo);
    },
    [activeRepo, createTargetRepo]
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
    async (branchName: string, customName: string, reuse: boolean, repo?: Repository) => {
      const targetRepo = repo ?? createTargetRepo ?? activeRepo;
      if (!targetRepo) return;

      const hasScript = hasStartupScript(targetRepo.id);

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
        const mainBranch = await getMainBranch(targetRepo.path);
        let wtPath: string;
        try {
          wtPath = await gitCreateWorktree(
            targetRepo.path,
            branchName,
            mainBranch,
            reuse
          );
        } catch (gitErr) {
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
          await syncWorktrees(targetRepo.id);
          await refreshRef.current();
          const { getWorktrees } = await import("./lib/db.js");
          const wts = getWorktrees(targetRepo.id);
          const newWt = wts.find((w) => w.branch === branchName);
          if (newWt) {
            updateWorktreeCustomName(newWt.id, customName);
          }
        }
        await syncWorktrees(targetRepo.id);
        await refreshRef.current();
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

        // Step: Run startup script
        if (hasScript && onRunScript) {
          updateStep(stepIdx, "active");
          await new Promise((r) => setTimeout(r, 300));
          onRunScript(getScriptPath(targetRepo.id), wtPath);
          exit();
          return;
        }

        // Clear create target repo
        setCreateTargetRepo(null);
        setMode("dashboard");
      } catch (err) {
        updateStep(stepIdx, "error");
        setCreationError(`${err}`);
        setTimeout(() => {
          setMode("dashboard");
          setError(`Failed to create worktree: ${err}`);
        }, 3000);
      }
    },
    [activeRepo, createTargetRepo, settings, onRunScript, exit, updateStep]
  );

  // Handle delete worktree
  const handleDelete = useCallback(async (options: DeleteOptions) => {
    const wt = flatWorktrees[selectedIndex];
    if (!wt || !activeRepo) return;

    // Find the repo for this worktree
    const wtRepo = repositories.find((r) => r.id === wt.repo_id) ?? activeRepo;

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
      await gitDeleteWorktree(wtRepo.path, wt.path, true);
      removeWorktreeDb(wt.id);
      updateDeleteStep(stepIdx, "done");
      stepIdx++;

      if (options.deleteLocalBranch) {
        updateDeleteStep(stepIdx, "active");
        try {
          await deleteBranch(wtRepo.path, wt.branch, true);
          updateDeleteStep(stepIdx, "done");
        } catch (err) {
          updateDeleteStep(stepIdx, "error");
          log("warn", "app", `Failed to delete local branch: ${err}`);
        }
        stepIdx++;
      }

      // Step: Sync database
      updateDeleteStep(stepIdx, "active");
      await refreshRef.current();
      updateDeleteStep(stepIdx, "done");

      setSelectedIndex((i) => Math.max(0, i - 1));
      log("info", "app", `Deleted worktree ${wt.branch}`);

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
  }, [flatWorktrees, selectedIndex, activeRepo, repositories, updateDeleteStep]);

  // Handle repo selection from folder browser
  const handleSelectFolder = useCallback(
    async (path: string) => {
      const name = getRepoName(path);
      const repo = addRepository(path, name);
      setRepositories(getRepositories());
      setMode("dashboard");

      setBusy("Syncing worktrees...");
      await syncWorktrees(repo.id);
      await refreshRef.current();
      setBusy(null);
    },
    []
  );

  // Handle repo switch (from RepoSelector in settings context)
  const handleSwitchRepo = useCallback(
    async (repo: Repository) => {
      touchRepository(repo.id);
      setMode("dashboard");
      setSelectedIndex(0);

      setBusy("Syncing worktrees...");
      await syncWorktrees(repo.id);
      await refreshRef.current();
      setBusy(null);
    },
    []
  );

  // Handle repo pick for create-worktree flow
  const handlePickRepoForCreate = useCallback(
    (repo: Repository) => {
      setCreateTargetRepo(repo);
      setMode("new-worktree");
    },
    []
  );

  // Handle settings save
  const handleSaveSettings = useCallback(
    (newSettings: Settings) => {
      setSettings(newSettings);
      saveSettings(newSettings);
    },
    []
  );

  // Handle factory reset
  const handleFactoryReset = useCallback(() => {
    resetAll();
    saveSettings(DEFAULT_SETTINGS);
    setSettings(DEFAULT_SETTINGS);
    setRepositories([]);
    setMode("folder-browse");
  }, []);

  // Handle remove repo from settings
  const handleRemoveRepo = useCallback(
    (repoId: string) => {
      removeRepository(repoId);
      const repos = getRepositories();
      setRepositories(repos);
      if (repos.length === 0) {
        setMode("folder-browse");
      }
    },
    []
  );

  // Key bindings for dashboard mode
  useKeyBindings({
    selectedIndex,
    worktreeCount: flatWorktrees.length,
    mode,
    busy,
    onSelect: setSelectedIndex,
    onEnter: handleOpen,
    onNew: () => {
      if (repositories.length > 1) {
        // Show repo picker first, then chain into new-worktree
        setCreateTargetRepo(null);
        setMode("repo-select");
      } else {
        setCreateTargetRepo(repositories[0] ?? null);
        setMode("new-worktree");
      }
    },
    onDelete: () => {
      if (flatWorktrees[selectedIndex]) setMode("delete-confirm");
    },
    onSettings: () => setMode("settings"),
    onRefresh: async () => {
      setBusy("Syncing worktrees...");
      await Promise.all(repositories.map((repo) => syncWorktrees(repo.id)));
      await refreshRef.current();
      setBusy(null);
    },
    onOpenPr: () => {
      const wt = flatWorktrees[selectedIndex];
      if (wt?.pr_info?.url) {
        import("open").then((mod) => mod.default(wt.pr_info!.url)).catch(() => {});
      }
    },
    onOpenLinear: () => {
      const wt = flatWorktrees[selectedIndex];
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
    <Box flexDirection="column" height={stdout?.rows ?? 24}>
      {error && (
        <Box paddingX={1}>
          <Text color="red">Error: {error}</Text>
        </Box>
      )}

      {mode === "folder-browse" && (
        <FolderBrowser
          onSelect={handleSelectFolder}
          onCancel={() => {
            if (repositories.length > 0) {
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
          onSelect={handlePickRepoForCreate}
          onCancel={() => setMode("dashboard")}
        />
      )}

      {mode === "new-worktree" && (
        <NewWorktreeForm
          defaultPrefix={settings.defaultBranchPrefix}
          onSubmit={handleCreate}
          onCancel={() => {
            setCreateTargetRepo(null);
            setMode("dashboard");
          }}
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
            const repo = createTargetRepo ?? activeRepo;
            if (!repo) return;
            const branch = pendingBranch.branch;
            const customName = pendingBranch.customName;
            setPendingBranch(null);
            try {
              await deleteBranch(repo.path, branch, true);
            } catch {
              // Branch may only exist on remote, ignore local delete failure
            }
            await doCreateWorktree(branch, customName, false, repo);
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

      {mode === "delete-confirm" && flatWorktrees[selectedIndex] && activeRepo && (
        <DeleteConfirm
          worktree={flatWorktrees[selectedIndex]}
          repoPath={activeRepo.path}
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
          onFactoryReset={handleFactoryReset}
        />
      )}

      {mode === "dashboard" && (
        <Dashboard
          repoName={activeRepo?.name ?? "No repository"}
          groups={groups}
          flatWorktrees={flatWorktrees}
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
