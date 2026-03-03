import { getWorktreeByPath, getAgentStatus } from "../lib/db.js";

export function printStatus(worktreePath?: string): void {
  if (!worktreePath) {
    console.log("Usage: am status --worktree <path>");
    process.exit(1);
  }

  const worktree = getWorktreeByPath(worktreePath);
  if (!worktree) {
    console.log(`No worktree found in DB for: ${worktreePath}`);
    process.exit(1);
  }

  const status = getAgentStatus(worktree.id);
  console.log(`Worktree: ${worktree.path}`);
  console.log(`Branch:   ${worktree.branch}`);
  console.log(`Status:   ${status?.status ?? "unknown"}`);
  console.log(`Session:  ${status?.session_id ?? "none"}`);
  console.log(`Updated:  ${status?.updated_at ?? "never"}`);
  if (status?.last_response) {
    const truncated =
      status.last_response.length > 200
        ? status.last_response.slice(0, 200) + "..."
        : status.last_response;
    console.log(`Last Response: ${truncated}`);
  }
}
