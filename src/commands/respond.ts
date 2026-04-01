import net from "net";
import { existsSync } from "fs";
import { SOCKET_PATH, DAEMON_PID_PATH } from "../lib/paths.js";
import { resolveWorktree } from "../lib/resolve.js";
import { getDb, getAllPendingInputs, getPendingInputForWorktree } from "../lib/db.js";
import type { PendingInput } from "../lib/types.js";

export async function respond(
  target: string,
  message: string | undefined,
  opts: { approve?: boolean; deny?: boolean; repo?: string }
): Promise<void> {
  getDb();

  const worktree = resolveWorktree(target, opts.repo);
  const pendingInput = getPendingInputForWorktree(worktree.id);

  if (!pendingInput) {
    // Check if there are any pending inputs at all
    const all = getAllPendingInputs();
    if (all.length === 0) {
      console.error("No pending inputs — no Claude agents are waiting for a response.");
    } else {
      console.error(`No pending input for worktree "${target}".`);
      console.error("Pending inputs:");
      for (const pi of all) {
        console.error(`  - ${pi.worktreeId}: ${pi.type} — ${pi.question?.slice(0, 80)}`);
      }
    }
    process.exit(1);
  }

  // Determine response
  let decision: "allow" | "deny" | undefined;
  let response = message ?? "";

  if (pendingInput.type === "permission") {
    if (opts.deny) {
      decision = "deny";
    } else {
      decision = "allow"; // Default to allow
    }
  }

  if (opts.approve) {
    decision = "allow";
  }

  // Send via daemon socket
  if (!existsSync(SOCKET_PATH) || !existsSync(DAEMON_PID_PATH)) {
    console.error("Daemon is not running. Start the TUI first, or run: am daemon start");
    process.exit(1);
  }

  const success = await sendToDaemon(pendingInput, response, decision);
  if (success) {
    if (pendingInput.type === "permission") {
      console.log(`${decision === "deny" ? "Denied" : "Approved"} permission for ${worktree.branch}`);
    } else {
      console.log(`Sent response to ${worktree.branch}: "${response}"`);
    }
  } else {
    console.error("Failed to send response to daemon.");
    process.exit(1);
  }
}

function sendToDaemon(
  pendingInput: PendingInput,
  response: string,
  decision?: "allow" | "deny"
): Promise<boolean> {
  return new Promise((resolve) => {
    const conn = net.createConnection({ path: SOCKET_PATH }, () => {
      const msg = {
        type: "send-response",
        inputId: pendingInput.id,
        response,
        decision,
      };
      conn.write(JSON.stringify(msg) + "\n");
      conn.end();
      resolve(true);
    });

    conn.on("error", () => resolve(false));
    conn.setTimeout(5000, () => {
      conn.destroy();
      resolve(false);
    });
  });
}
