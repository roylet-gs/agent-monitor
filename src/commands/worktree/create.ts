import {
  createWorktree,
  localBranchExists,
  lsRemoteBranch,
  getMainBranch,
  fetchBranch,
  fetchAndResetBranch,
  remoteBranchExists,
  deleteBranch,
} from "../../lib/git.js";
import { syncWorktrees } from "../../lib/sync.js";
import { resolveRepo } from "../../lib/resolve.js";
import { installGlobalHooks, isGlobalHooksInstalled } from "../../lib/hooks-installer.js";
import { outputJson } from "../../lib/output.js";

export async function worktreeCreate(
  branch: string,
  opts: {
    repo?: string;
    name?: string;
    base?: string;
    reuse?: boolean;
    track?: boolean;
    noTrack?: boolean;
    json?: boolean;
  }
): Promise<void> {
  const repo = resolveRepo(opts.repo);

  const [localExists, remoteExists] = await Promise.all([
    localBranchExists(repo.path, branch),
    lsRemoteBranch(repo.path, branch),
  ]);

  // Validate the chosen mode against current state.
  if (opts.reuse && opts.track) {
    console.error(`--reuse and --track are mutually exclusive.`);
    process.exit(1);
  }
  if (opts.track && opts.noTrack) {
    console.error(`--track and --no-track are mutually exclusive.`);
    process.exit(1);
  }
  if (opts.track && !remoteExists) {
    console.error(`Cannot --track: branch "${branch}" does not exist on origin.`);
    process.exit(1);
  }

  const noFlag = !opts.reuse && !opts.track && !opts.noTrack;
  if (noFlag && (localExists || remoteExists)) {
    const where = localExists && remoteExists
      ? "locally and on origin"
      : remoteExists
        ? "on origin"
        : "locally";
    const hint = remoteExists
      ? "Use --track to pull remote, --no-track to create disconnected, or --reuse for the existing local branch."
      : "Use --reuse to attach to the existing branch.";
    console.error(`Branch "${branch}" already exists ${where}. ${hint}`);
    process.exit(1);
  }

  const baseBranch = opts.base ?? (await getMainBranch(repo.path));
  let createdMode: "fresh" | "reuse" | "track" | "no-track" = "fresh";

  let worktreePath: string;
  if (opts.reuse) {
    await fetchAndResetBranch(repo.path, branch);
    worktreePath = await createWorktree(repo.path, branch, { reuse: true });
    createdMode = "reuse";
  } else if (opts.track) {
    // Pull remote: if local exists, fetch + reset (which also sets upstream)
    // and reuse. Otherwise create a new local branch tracking origin/<branch>.
    if (localExists) {
      await fetchAndResetBranch(repo.path, branch);
      worktreePath = await createWorktree(repo.path, branch, { reuse: true });
    } else {
      await fetchBranch(repo.path, branch);
      worktreePath = await createWorktree(repo.path, branch, {
        baseRef: `origin/${branch}`,
        track: true,
      });
    }
    createdMode = "track";
  } else if (opts.noTrack) {
    if (localExists) {
      await deleteBranch(repo.path, branch, true);
    }
    await fetchBranch(repo.path, baseBranch);
    const baseHasRemote = await remoteBranchExists(repo.path, baseBranch);
    const baseRef = baseHasRemote ? `origin/${baseBranch}` : baseBranch;
    worktreePath = await createWorktree(repo.path, branch, { baseRef, noTrack: true });
    createdMode = "no-track";
  } else {
    await fetchBranch(repo.path, baseBranch);
    const baseHasRemote = await remoteBranchExists(repo.path, baseBranch);
    const baseRef = baseHasRemote ? `origin/${baseBranch}` : baseBranch;
    worktreePath = await createWorktree(repo.path, branch, { baseRef });
  }

  await syncWorktrees(repo.id);

  if (!isGlobalHooksInstalled()) {
    installGlobalHooks();
    console.log("Claude hooks installed automatically.");
  }

  if (opts.json) {
    outputJson({ branch, path: worktreePath, repo: repo.name, mode: createdMode });
  } else {
    console.log(`Created worktree: ${worktreePath}`);
    const desc =
      createdMode === "track"
        ? `${branch} (tracking origin/${branch})`
        : createdMode === "no-track"
          ? `${branch} (new, not tracking remote — based on ${baseBranch})`
          : createdMode === "reuse"
            ? `${branch} (reused existing)`
            : `${branch} (based on ${baseBranch})`;
    console.log(`Branch: ${desc}`);
  }
}
