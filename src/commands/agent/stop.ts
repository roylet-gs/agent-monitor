import { resolveWorktree, resolveRepo } from "../../lib/resolve.js";
import { getManagedSession } from "../../lib/db.js";
import { stopTurn } from "../../lib/claude-session.js";

export function agentStop(target: string, opts: { repo?: string }): void {
  const repoId = opts.repo ? resolveRepo(opts.repo).id : undefined;
  const worktree = resolveWorktree(target, repoId);
  const session = getManagedSession(worktree.id);
  if (!session) {
    console.error(`No managed session for ${worktree.branch}.`);
    process.exit(1);
  }

  if (stopTurn(session)) {
    console.log(`Stopped in-flight turn for ${worktree.custom_name ?? worktree.branch}.`);
  } else {
    console.log("No turn is currently running.");
  }
}
