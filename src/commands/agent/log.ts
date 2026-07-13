import { existsSync, readFileSync } from "fs";
import { resolveWorktree, resolveRepo } from "../../lib/resolve.js";
import { getManagedSession, getAgentStatus } from "../../lib/db.js";
import { loadTranscript, claudeTranscriptPath, sessionLogPath } from "../../lib/claude-session.js";

export function agentLog(target: string, opts: { repo?: string; json?: boolean }): void {
  const repoId = opts.repo ? resolveRepo(opts.repo).id : undefined;
  const worktree = resolveWorktree(target, repoId);
  // Managed session first, then any session hooks observed at this worktree
  const sessionId = getManagedSession(worktree.id)?.id ?? getAgentStatus(worktree.id)?.session_id;
  if (!sessionId) {
    console.error(`No Claude session known for ${worktree.branch}. Start one with: am agent send ${target} "<prompt>"`);
    process.exit(1);
  }

  if (opts.json) {
    const claudeFile = claudeTranscriptPath(worktree.path, sessionId);
    const raw = existsSync(claudeFile) ? claudeFile : sessionLogPath(sessionId);
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
