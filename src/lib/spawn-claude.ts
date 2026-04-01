import { spawn } from "child_process";
import { getWorktreeById, getAgentStatus } from "./db.js";
import { log } from "./logger.js";

/**
 * Spawn a headless `claude -p` process for a worktree.
 * Resumes the existing session if one exists, otherwise starts a new conversation.
 *
 * Works both from the daemon and from the TUI (fallback mode).
 */
export function spawnClaudeHeadless(
  worktreeId: string,
  message: string,
  onResult?: (success: boolean, error?: string) => void
): void {
  const worktree = getWorktreeById(worktreeId);
  if (!worktree) {
    log("warn", "spawn-claude", `Worktree ${worktreeId} not found`);
    onResult?.(false, "Worktree not found");
    return;
  }

  const agentStatus = getAgentStatus(worktreeId);
  const sessionId = agentStatus?.session_id ?? null;

  const args = sessionId
    ? ["--resume", sessionId, "-p", message]
    : ["-p", message];

  log("info", "spawn-claude", `Spawning claude in ${worktree.path} sessionId=${sessionId} args=${args.join(" ").slice(0, 200)}`);

  try {
    const child = spawn("claude", args, {
      cwd: worktree.path,
      detached: true,
      stdio: ["ignore", "pipe", "pipe"],
      env: { ...process.env },
    });

    let stderr = "";
    child.stderr?.on("data", (chunk: Buffer) => { stderr += chunk.toString(); });
    child.on("error", (err) => {
      log("error", "spawn-claude", `Spawn error: ${err.message}`);
      onResult?.(false, err.message);
    });
    child.on("exit", (code) => {
      if (code !== 0) {
        log("warn", "spawn-claude", `claude exited with code ${code}: ${stderr.slice(0, 500)}`);
      } else {
        log("info", "spawn-claude", `claude completed for ${worktreeId}`);
      }
    });

    child.unref();
    onResult?.(true);
  } catch (err) {
    log("error", "spawn-claude", `Failed to spawn claude: ${err}`);
    onResult?.(false, String(err));
  }
}
