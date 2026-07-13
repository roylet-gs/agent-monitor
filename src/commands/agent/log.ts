import { existsSync, readFileSync } from "fs";
import { resolveWorktree, resolveRepo } from "../../lib/resolve.js";
import { getManagedSession, getAgentStatus } from "../../lib/db.js";
import { loadTranscript, findClaudeTranscript, sessionLogPath } from "../../lib/claude-session.js";

export async function agentLog(target: string, opts: { repo?: string; json?: boolean; session?: string }): Promise<void> {
  const repoId = opts.repo ? resolveRepo(opts.repo).id : undefined;
  const worktree = resolveWorktree(target, repoId);
  // Explicit --session first, then the managed session, then any session
  // hooks observed at this worktree
  let sessionId: string | undefined;
  if (opts.session) {
    const { resolveSessionId } = await import("./sessions.js");
    sessionId = resolveSessionId(worktree.path, opts.session).id;
  } else {
    sessionId = getManagedSession(worktree.id)?.id ?? getAgentStatus(worktree.id)?.session_id ?? undefined;
  }
  if (!sessionId) {
    console.error(`No Claude session known for ${worktree.branch}. Start one with: am agent send ${target} "<prompt>"`);
    process.exit(1);
  }

  if (opts.json) {
    const raw = findClaudeTranscript(worktree.path, sessionId) ?? sessionLogPath(sessionId);
    process.stdout.write(existsSync(raw) ? readFileSync(raw, "utf-8") : "");
    return;
  }

  const transcript = loadTranscript(worktree.path, sessionId);
  if (transcript.length === 0) {
    console.log("(empty transcript)");
    return;
  }
  for (const msg of transcript) {
    switch (msg.role) {
      case "user":
        console.log(`\n> ${msg.text}`);
        break;
      case "assistant":
        console.log(`\n${msg.text}`);
        break;
      case "tool":
        console.log(`  ⚒ ${msg.text}`);
        break;
      case "system":
        console.log(`  · ${msg.text}`);
        break;
      case "error":
        console.log(`  ✗ ${msg.text}`);
        break;
    }
  }
}
