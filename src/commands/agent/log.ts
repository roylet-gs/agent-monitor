import { existsSync, readFileSync } from "fs";
import { resolveWorktree, resolveRepo } from "../../lib/resolve.js";
import { getManagedSession } from "../../lib/db.js";
import { parseTranscript, sessionLogPath } from "../../lib/claude-session.js";

export function agentLog(target: string, opts: { repo?: string; json?: boolean }): void {
  const repoId = opts.repo ? resolveRepo(opts.repo).id : undefined;
  const worktree = resolveWorktree(target, repoId);
  const session = getManagedSession(worktree.id);
  if (!session) {
    console.error(`No managed session for ${worktree.branch}. Start one with: am agent send ${target} "<prompt>"`);
    process.exit(1);
  }

  if (opts.json) {
    const logPath = sessionLogPath(session.id);
    process.stdout.write(existsSync(logPath) ? readFileSync(logPath, "utf-8") : "");
    return;
  }

  const transcript = parseTranscript(session.id);
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
