import net from "net";
import { existsSync } from "fs";
import { SOCKET_PATH, DAEMON_PID_PATH } from "../lib/paths.js";
import { resolveWorktree } from "../lib/resolve.js";
import { getDb } from "../lib/db.js";

export async function sendPrompt(
  target: string,
  message: string,
  opts: { repo?: string }
): Promise<void> {
  getDb();

  const worktree = resolveWorktree(target, opts.repo);

  if (!message.trim()) {
    console.error("Message cannot be empty.");
    process.exit(1);
  }

  // Send via daemon socket
  if (!existsSync(SOCKET_PATH) || !existsSync(DAEMON_PID_PATH)) {
    console.error("Daemon is not running. Start the TUI first, or run: am daemon start");
    process.exit(1);
  }

  const success = await sendToDaemon(worktree.id, message);
  if (success) {
    console.log(`Sent prompt to ${worktree.branch}: "${message.slice(0, 80)}${message.length > 80 ? "..." : ""}"`);
  } else {
    console.error("Failed to send prompt to daemon.");
    process.exit(1);
  }
}

function sendToDaemon(worktreeId: string, message: string): Promise<boolean> {
  return new Promise((resolve) => {
    const conn = net.createConnection({ path: SOCKET_PATH }, () => {
      const msg = {
        type: "send-prompt",
        worktreeId,
        message,
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
