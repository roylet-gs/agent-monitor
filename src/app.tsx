import React, { useState, useEffect, useCallback, useRef, useMemo } from "react";
import { Box, Text, useApp, useInput, useStdout } from "ink";
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
import { usePubSub } from "./hooks/usePubSub.js";
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
  fetchBranch,
} from "./lib/git.js";
import { syncWorktrees } from "./lib/sync.js";
import { installGlobalHooks, isGlobalHooksInstalled } from "./lib/hooks-installer.js";
import { openInIde, openTerminal, openClaudeInTerminal } from "./lib/ide-launcher.js";
import { hasStartupScript, getScriptPath } from "./lib/scripts.js";
import { loadSettings, saveSettings, DEFAULT_SETTINGS, isFirstRun } from "./lib/settings.js";
import { useUpdateCheck } from "./hooks/useUpdateCheck.js";
import { log } from "./lib/logger.js";
import { getVersion, isNewVersion } from "./lib/version.js";
import { SetupWizard } from "./components/SetupWizard.js";
import type { AppMode, Repository, Settings } from "./lib/types.js";

interface AppProps {
  onRunScript?: (scriptPath: string, cwd: string) => void;
  watch?: boolean;
  onUpdate?: () => void;
  forceSetup?: boolean;
}

export function App({ onRunScript, watch, onUpdate, forceSetup }: AppProps) {
  const { exit } = useApp();
  const { stdout } = useStdout();
  const [settings, setSettings] = useState<Settings>(loadSettings);
  const [mode, setMode] = useState<AppMode>("dashboard");
  const [busy, setBusy] = useState<string | null>(null);
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [repositories, setRepositories] = useState<Repository[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [showLogs, setShowLogs] = useState(watch ?? false);
  const [escHint, setEscHint] = useState(false);
  const [pendingBranch, setPendingBranch] = useState<{ branch: string; customName: string; baseBranch: string } | null>(null);
  const [creatingBranch, setCreatingBranch] = useState("");
  const [creationSteps, setCreationSteps] = useState<StepInfo[]>([]);
  const [creationError, setCreationError] = useState<string | null>(null);
  const [deletingBranch, setDeletingBranch] = useState("");
  const [deleteSteps, setDeleteSteps] = useState<StepInfo[]>([]);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const [deleteRecovery, setDeleteRecovery] = useState<{
    worktreeId: string;
    repoPath: string;
    branch: string;
    originalOptions: DeleteOptions;
    errorMessage: string;
  } | null>(null);
  // For create-worktree flow: repo picked from RepoSelector
  const [createTargetRepo, setCreateTargetRepo] = useState<Repository | null>(null);
  const [currentVersion] = useState(() => getVersion());

  // Initialize DB and check for repos
  useEffect(() => {
    getDb();
    const repos = getRepositories();
    setRepositories(repos);

    // Check first-run BEFORE version check writes settings.json
    const firstRun = isFirstRun() || forceSetup;

    if (settings.lastSeenVersion === undefined || isNewVersion(settings.lastSeenVersion, currentVersion)) {
      // Silently record current version
      const updated = { ...settings, lastSeenVersion: currentVersion };
      setSettings(updated);
      saveSettings(updated);
    }

    if (firstRun) {
      setMode("setup");
    } else if (repos.length === 0) {
      setMode("folder-browse");
    }
  }, []);

  // Sync all repos on startup if enabled
  useEffect(() => {
    if (repositories.length > 0 && settings.autoSyncOnStartup) {
      setBusy("Syncing worktrees...");
      Promise.all(repositories.map((repo) => syncWorktrees(repo.id)))
        .then(() => lightRefreshRef.current())
        .then(() => setBusy(null))
        .catch((err) => {
          log("error", "app", `Sync failed: ${err}`);
          setBusy(null);
        });
    }
  }, [repositories.length > 0 && settings.autoSyncOnStartup]);

  const { groups, flatWorktrees, refresh, lightRefresh } = useWorktrees({
    repositories,
    pollingIntervalMs: settings.pollingIntervalMs,
    ghPollingIntervalMs: settings.ghPollingIntervalMs,
    linearPollingIntervalMs: settings.linearPollingIntervalMs,
    ghPrStatus: settings.ghPrStatus,
    linearEnabled: settings.linearEnabled,
    linearApiKey: settings.linearApiKey,
    hideMainBranch: settings.hideMainBranch,
    ghRefreshOnManual: settings.ghRefreshOnManual,
    linearRefreshOnManual: settings.linearRefreshOnManual,
    linearAutoNickname: settings.linearAutoNickname,
  });

  // Keep refs to always call the latest refresh (avoids stale closures in async handlers)
  const refreshRef = useRef(refresh);
  useEffect(() => { refreshRef.current = refresh; }, [refresh]);
  const lightRefreshRef = useRef(lightRefresh);
  useEffect(() => { lightRefreshRef.current = lightRefresh; }, [lightRefresh]);

  // Pub/sub: instant refresh on agent status updates
  usePubSub((msg) => {
    if (msg.type === "agent-status-update") {
      // Light refresh only: re-read DB + git status without triggering GitHub/Linear API calls
      lightRefreshRef.current();
    } else if (msg.type === "git-activity") {
      // Git push or PR creation detected — force full refresh (with integrations) after a short
      // delay to give GitHub time to process the push/PR.
      log("info", "app", `Git activity detected: ${msg.activity} on ${msg.branch}`);
      setTimeout(() => refreshRef.current(), 3000);
    }
  });

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

  // Auto-install global hooks on startup if not present (skip during setup wizard)
  useEffect(() => {
    if (mode !== "setup" && !isGlobalHooksInstalled()) {
      installGlobalHooks();
    }
  }, []);

  // Handle open in IDE
  const handleOpen = useCallback(async () => {
    const wt = flatWorktrees[selectedIndex];
    if (!wt) return;

    try {
      openInIde(wt.path, settings.ide);
    } catch (err) {
      setError(`${err}`);
    }
  }, [flatWorktrees, selectedIndex, settings]);

  // Handle open terminal at worktree path
  const handleOpenTerminal = useCallback(() => {
    const wt = flatWorktrees[selectedIndex];
    if (!wt) return;
    try {
      openTerminal(wt.path);
    } catch (err) {
      setError(`${err}`);
    }
  }, [flatWorktrees, selectedIndex]);

  // Handle open Claude in a new terminal window
  const handleOpenClaude = useCallback(() => {
    const wt = flatWorktrees[selectedIndex];
    if (!wt) return;

    const continueSession = !!wt.agent_status?.session_id;
    try {
      openClaudeInTerminal(wt.path, continueSession);
    } catch (err) {
      setError(`${err}`);
    }
  }, [flatWorktrees, selectedIndex]);

  // Handle create worktree
  const handleCreate = useCallback(
    async (branchName: string, customName: string, baseBranch: string) => {
      const repo = createTargetRepo ?? activeRepo;
      if (!repo) return;

      const exists = await branchExists(repo.path, branchName);
      if (exists) {
        setPendingBranch({ branch: branchName, customName, baseBranch });
        setMode("branch-exists");
        return;
      }

      const { existsSync } = await import("fs");
      const { join } = await import("path");
      const wtDir = join(repo.path, ".claude", "worktrees", branchName.replace(/\//g, "-"));
      if (existsSync(wtDir)) {
        setPendingBranch({ branch: branchName, customName, baseBranch });
        setMode("branch-exists");
        return;
      }

      await doCreateWorktree(branchName, customName, false, repo, baseBranch);
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
    async (branchName: string, customName: string, reuse: boolean, repo?: Repository, baseBranch?: string) => {
      const targetRepo = repo ?? createTargetRepo ?? activeRepo;
      if (!targetRepo) return;

      const hasScript = hasStartupScript(targetRepo.id);

      const steps: StepInfo[] = [
        { label: "Fetching latest base branch", status: "active" },
        { label: "Creating git worktree", status: "pending" },
        { label: "Syncing database", status: "pending" },
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
        // Step: Fetch base branch
        const effectiveBase = baseBranch?.trim() || await getMainBranch(targetRepo.path);
        await fetchBranch(targetRepo.path, effectiveBase);
        updateStep(stepIdx, "done");
        stepIdx++;

        // Step: Create worktree
        updateStep(stepIdx, "active");
        const baseRef = `origin/${effectiveBase}`;
        let wtPath: string;
        try {
          wtPath = await gitCreateWorktree(
            targetRepo.path,
            branchName,
            baseRef,
            reuse
          );
        } catch (gitErr) {
          const errMsg = String(gitErr);
          if (errMsg.includes("already exists") || errMsg.includes("already checked out")) {
            updateStep(stepIdx, "error");
            setCreationError(null);
            setMode("dashboard");
            setPendingBranch({ branch: branchName, customName, baseBranch: baseBranch ?? effectiveBase });
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

    // Safety guard: never allow deleting the main working tree
    if (wt.path === wtRepo.path) {
      setError("Cannot delete the main working tree");
      return;
    }

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
    setDeleteRecovery(null);
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
      const errorMessage = `${err}`;
      setDeleteError(errorMessage);

      // Build recovery steps to show what will happen
      const recoverySteps: StepInfo[] = [
        { label: "Removing worktree", status: "error" as const },
        { label: "Remove from database", status: "pending" as const },
        ...(options.deleteLocalBranch
          ? [{ label: `Delete local branch ${wt.branch}`, status: "pending" as const }]
          : []),
        { label: "Syncing database", status: "pending" as const },
      ];
      setDeleteSteps(recoverySteps);

      setDeleteRecovery({
        worktreeId: wt.id,
        repoPath: wtRepo.path,
        branch: wt.branch,
        originalOptions: options,
        errorMessage,
      });
      log("warn", "app", `Worktree removal failed, offering recovery: ${errorMessage}`);
    }
  }, [flatWorktrees, selectedIndex, activeRepo, repositories, updateDeleteStep]);

  // Handle delete recovery (clean up DB + optionally delete branch)
  const handleDeleteRecovery = useCallback(async (shouldDeleteBranch: boolean) => {
    if (!deleteRecovery) return;

    const { worktreeId, repoPath, branch } = deleteRecovery;
    let stepIdx = 1; // Start after the failed "Removing worktree" step

    try {
      // Step: Remove from database
      updateDeleteStep(stepIdx, "active");
      removeWorktreeDb(worktreeId);
      updateDeleteStep(stepIdx, "done");
      stepIdx++;

      // Step: Delete branch (if requested)
      if (shouldDeleteBranch) {
        updateDeleteStep(stepIdx, "active");
        try {
          await deleteBranch(repoPath, branch, true);
          updateDeleteStep(stepIdx, "done");
        } catch (err) {
          updateDeleteStep(stepIdx, "error");
          log("warn", "app", `Failed to delete local branch during recovery: ${err}`);
        }
        stepIdx++;
      }

      // Step: Sync database
      updateDeleteStep(stepIdx, "active");
      await refreshRef.current();
      updateDeleteStep(stepIdx, "done");

      setSelectedIndex((i) => Math.max(0, i - 1));
      log("info", "app", `Recovery cleanup completed for ${branch}`);

      setDeleteRecovery(null);
      setDeleteError(null);
      await new Promise((r) => setTimeout(r, 500));
      setMode("dashboard");
    } catch (err) {
      updateDeleteStep(stepIdx, "error");
      setDeleteError(`Recovery failed: ${err}`);
      setTimeout(() => {
        setDeleteRecovery(null);
        setMode("dashboard");
        setError(`Recovery failed: ${err}`);
      }, 3000);
    }
  }, [deleteRecovery, updateDeleteStep]);

  // Input handler for delete recovery prompt
  useInput((input, key) => {
    if (mode !== "deleting-worktree" || !deleteRecovery) return;

    if (key.escape) {
      setDeleteRecovery(null);
      setDeleteError(null);
      setMode("dashboard");
      return;
    }

    if (key.return || input === "y") {
      handleDeleteRecovery(deleteRecovery.originalOptions.deleteLocalBranch);
      return;
    }

    if (input === "n") {
      handleDeleteRecovery(false);
      return;
    }
  }, { isActive: mode === "deleting-worktree" && deleteRecovery !== null });

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

  // Check for updates
  const { updateInfo, recheck } = useUpdateCheck(settings, handleSaveSettings);

  // Handle settings reset (back to defaults + setup wizard)
  const handleSettingsReset = useCallback(() => {
    saveSettings(DEFAULT_SETTINGS);
    setSettings(DEFAULT_SETTINGS);
    setMode("setup");
  }, []);

  // Handle factory reset
  const handleFactoryReset = useCallback(() => {
    resetAll();
    saveSettings(DEFAULT_SETTINGS);
    setSettings(DEFAULT_SETTINGS);
    setRepositories([]);
    setMode("setup");
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
        import("open").then((mod) => mod.default(wt.pr_info!.url)).catch((err) => {
          log("warn", "app", `Failed to open PR URL: ${err}`);
        });
      }
    },
    onOpenLinear: () => {
      const wt = flatWorktrees[selectedIndex];
      if (wt?.linear_info?.url) {
        let url = wt.linear_info!.url;
        if (settings.linearUseDesktopApp) {
          url = url.replace("https://linear.app/", "linear://");
        }
        import("open").then((mod) => mod.default(url)).catch(() => {});
      }
    },
    onOpenTerminal: handleOpenTerminal,
    onToggleLogs: () => setShowLogs((v) => !v),
    onUpdate: updateInfo?.updateAvailable
      ? () => {
          onUpdate?.();
          exit();
        }
      : undefined,
    onClaude: handleOpenClaude,
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

      {mode === "setup" && (
        <SetupWizard
          initialSettings={settings}
          onComplete={(newSettings, repoPath) => {
            handleSaveSettings({ ...newSettings, setupCompleted: true });
            if (repoPath) {
              handleSelectFolder(repoPath);
            } else {
              setMode("folder-browse");
            }
          }}
          onSkip={() => {
            handleSaveSettings({ ...settings, setupCompleted: true });
            if (repositories.length === 0) {
              setMode("folder-browse");
            } else {
              setMode("dashboard");
            }
          }}
        />
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
          defaultBaseBranch={settings.defaultBaseBranch}
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
            doCreateWorktree(pendingBranch.branch, pendingBranch.customName, true, undefined, pendingBranch.baseBranch);
            setPendingBranch(null);
          }}
          onDeleteAndRecreate={async () => {
            const repo = createTargetRepo ?? activeRepo;
            if (!repo) return;
            const branch = pendingBranch.branch;
            const customName = pendingBranch.customName;
            const baseBranch = pendingBranch.baseBranch;
            setPendingBranch(null);
            try {
              await deleteBranch(repo.path, branch, true);
            } catch (err) {
              log("debug", "app", `Local branch delete failed (may only exist on remote): ${err}`);
            }
            await doCreateWorktree(branch, customName, false, repo, baseBranch);
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
          prompt={deleteRecovery ? (
            <Box flexDirection="column">
              <Text>Clean up database entry{deleteRecovery.originalOptions.deleteLocalBranch ? ` and delete local branch ${deleteRecovery.branch}` : ""}?</Text>
              <Box marginTop={1}>
                <Text>
                  <Text color="yellow">[Enter/y]</Text> Yes{deleteRecovery.originalOptions.deleteLocalBranch ? " (remove DB + delete branch)" : " (remove from DB)"}{" "}
                  <Text color="yellow">[n]</Text> Remove from DB only{" "}
                  <Text color="yellow">[Esc]</Text> Cancel
                </Text>
              </Box>
            </Box>
          ) : undefined}
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
          onSettingsReset={handleSettingsReset}
          onFactoryReset={handleFactoryReset}
          onCheckForUpdates={recheck}
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
          showLogs={showLogs}
          terminalRows={stdout?.rows ?? 24}
          version={currentVersion}
          updateInfo={updateInfo}
          ghPrStatus={settings.ghPrStatus}
          linearEnabled={settings.linearEnabled}
        />
      )}
    </Box>
  );
}
