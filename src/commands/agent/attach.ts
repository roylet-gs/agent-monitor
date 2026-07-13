import { spawnSync } from "child_process";
import { resolveWorktree, resolveRepo } from "../../lib/resolve.js";
import { getManagedSession } from "../../lib/db.js";
import { isTurnRunning } from "../../lib/claude-session.js";

export function agentAttach(target: string, opts: { repo?: string; force?: boolean }): void {
  const repoId = opts.repo ? resolveRepo(opts.repo).id : undefined;
  const worktree = resolveWorktree(target, repoId);
  const session = getManagedSession(worktree.id);
  if (!session) {
    console.error(`No managed session for ${worktree.branch}. Start one with: am agent send ${target} "<prompt>"`);
    process.exit(1);
  }

  if (isTurnRunning(session) && !opts.force) {
    console.error(
      "A headless turn is still running for this session. Attaching now could conflict with it.\n" +
        `Wait for it to finish, stop it (am agent stop ${target}), or re-run with --force.`
    );
    process.exit(1);
  }

  const result = spawnSync("claude", ["--resume", session.id], {
    cwd: worktree.path,
    stdio: "inherit",
  });
  process.exit(result.status ?? 0);
}
