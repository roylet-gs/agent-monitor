import { resolveWorktree, resolveRepo } from "../../lib/resolve.js";
import { getManagedSession } from "../../lib/db.js";
import { discoverWorktreeSessions, type DiscoveredSession } from "../../lib/claude-session.js";
import { outputJson, outputTable } from "../../lib/output.js";

export function agentSessions(target: string, opts: { repo?: string; json?: boolean }): void {
  const repoId = opts.repo ? resolveRepo(opts.repo).id : undefined;
  const worktree = resolveWorktree(target, repoId);
  const sessions = discoverWorktreeSessions(worktree.path);
  const activeId = getManagedSession(worktree.id)?.id ?? null;

  if (opts.json) {
    outputJson(sessions.map((s) => ({ ...s, active: s.id === activeId })));
    return;
  }

  const rows = sessions.map((s) => ({
    session: s.id.slice(0, 8),
    dir: s.cwd === worktree.path ? "." : s.cwd.slice(worktree.path.length + 1),
    modified: new Date(s.mtimeMs).toISOString().replace("T", " ").slice(0, 16),
    active: s.id === activeId ? "✓" : "",
    lastPrompt: s.lastPrompt ?? "",
  }));

  outputTable(rows, [
    { key: "session", header: "SESSION" },
    { key: "dir", header: "DIR" },
    { key: "modified", header: "MODIFIED" },
    { key: "active", header: "ACTIVE" },
    { key: "lastPrompt", header: "LAST PROMPT", width: 50 },
  ]);
}

/** Resolve a --session id or prefix against the worktree's discovered sessions. */
export function resolveSessionId(worktreePath: string, idOrPrefix: string): DiscoveredSession {
  const matches = discoverWorktreeSessions(worktreePath).filter((s) => s.id.startsWith(idOrPrefix));
  if (matches.length === 1) return matches[0]!;
  if (matches.length === 0) {
    console.error(`No session matching "${idOrPrefix}" at this worktree. See: am agent sessions <target>`);
  } else {
    console.error(`"${idOrPrefix}" matches ${matches.length} sessions — use more characters.`);
  }
  process.exit(1);
}
