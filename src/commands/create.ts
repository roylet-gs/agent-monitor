import { resolve } from "path";
import { getRepositories, getRepositoryByPath, updateWorktreeCustomName } from "../lib/db.js";
import {
  branchExists,
  getMainBranch,
  createWorktree,
} from "../lib/git.js";
import { syncWorktrees } from "../lib/sync.js";
import { installHooks } from "../lib/hooks-installer.js";
import { openInIde } from "../lib/ide-launcher.js";
import { loadSettings } from "../lib/settings.js";

export interface CreateFlags {
  repo?: string;
  name?: string;
  reuse?: boolean;
  noHooks?: boolean;
  open?: boolean;
}

export async function runCreate(branch: string, flags: CreateFlags): Promise<void> {
  const settings = loadSettings();
  let repos = getRepositories();

  if (flags.repo) {
    const absPath = resolve(flags.repo);
    const repo = getRepositoryByPath(absPath);
    if (!repo) {
      console.error(`Error: repository not found: ${absPath}`);
      process.exit(1);
    }
    repos = [repo];
  }

  if (repos.length === 0) {
    console.error("Error: no repositories added. Use: am repo add <path>");
    process.exit(1);
  }

  if (repos.length > 1) {
    console.error("Error: multiple repositories found. Use --repo to specify which one:");
    for (const r of repos) {
      console.error(`  ${r.name}\t${r.path}`);
    }
    process.exit(1);
  }

  const repo = repos[0]!;
  const exists = await branchExists(repo.path, branch);

  if (exists && !flags.reuse) {
    console.error(`Error: branch "${branch}" already exists. Use --reuse to checkout the existing branch.`);
    process.exit(1);
  }

  const mainBranch = await getMainBranch(repo.path);
  const worktreePath = await createWorktree(
    repo.path,
    branch,
    exists ? undefined : mainBranch,
    exists
  );

  console.log(`Created worktree at ${worktreePath}`);

  await syncWorktrees(repo.id);

  // Set custom name if provided
  if (flags.name) {
    const { getWorktreeByBranch } = await import("../lib/db.js");
    const wt = getWorktreeByBranch(repo.id, branch);
    if (wt) {
      updateWorktreeCustomName(wt.id, flags.name);
    }
  }

  // Install hooks unless --no-hooks
  if (!flags.noHooks && settings.autoInstallHooks) {
    installHooks(worktreePath);
    console.log("Hooks installed.");
  }

  // Open in IDE if --open
  if (flags.open) {
    openInIde(worktreePath, settings.ide);
    console.log(`Opened in ${settings.ide}.`);
  }
}
