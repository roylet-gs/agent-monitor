import { getWorktreeByPath, getAgentStatus, upsertAgentStatus } from "../lib/db.js";
import { publishMessage } from "../lib/pubsub-client.js";
import type { AgentStatusType } from "../lib/types.js";

const VALID_STATUSES: AgentStatusType[] = ["idle", "executing", "planning", "waiting"];

export async function printStatus(worktreePath?: string, setStatus?: string): Promise<void> {
  if (!worktreePath) {
    console.log("Usage: am status --worktree <path> [--set <status>]");
    process.exit(1);
  }

  const worktree = getWorktreeByPath(worktreePath);
  if (!worktree) {
    console.log(`No worktree found in DB for: ${worktreePath}`);
    process.exit(1);
  }

  // Write mode: --set <status>
  if (setStatus) {
    if (!VALID_STATUSES.includes(setStatus as AgentStatusType)) {
      console.log(`Invalid status: ${setStatus}. Must be one of: ${VALID_STATUSES.join(", ")}`);
      process.exit(1);
    }
    const status = setStatus as AgentStatusType;
    upsertAgentStatus(worktree.id, status, null, null, null);

    await publishMessage({
      type: "agent-status-update",
      worktreeId: worktree.id,
      status,
      sessionId: null,
      lastResponse: null,
      transcriptSummary: null,
      updatedAt: new Date().toISOString(),
    }).catch(() => {});

    console.log(`Status set to "${status}" for ${worktree.path}`);
    return;
  }

  // Read mode (default)
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
