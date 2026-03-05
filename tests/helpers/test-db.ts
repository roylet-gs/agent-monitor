/**
 * Test helper: seed database with repos, worktrees, and agent statuses.
 */

interface SeedRepo {
  path: string;
  name: string;
}

interface SeedWorktree {
  repoIndex: number;
  path: string;
  branch: string;
  name: string;
}

interface SeedAgentStatus {
  worktreeIndex: number;
  status: "idle" | "executing" | "planning" | "waiting";
  sessionId?: string;
  lastResponse?: string;
  transcriptSummary?: string;
}

interface SeedResult {
  repos: Array<{ id: string; path: string; name: string }>;
  worktrees: Array<{ id: string; repo_id: string; path: string; branch: string; name: string }>;
}

export async function seedDatabase(options: {
  repos?: SeedRepo[];
  worktrees?: SeedWorktree[];
  agentStatuses?: SeedAgentStatus[];
}): Promise<SeedResult> {
  const { addRepository, upsertWorktree, upsertAgentStatus } = await import(
    "../../src/lib/db.js"
  );

  const repos: SeedResult["repos"] = [];
  const worktrees: SeedResult["worktrees"] = [];

  for (const r of options.repos ?? []) {
    const repo = addRepository(r.path, r.name);
    repos.push(repo);
  }

  for (const w of options.worktrees ?? []) {
    const repo = repos[w.repoIndex]!;
    const wt = upsertWorktree(repo.id, w.path, w.branch, w.name);
    worktrees.push(wt);
  }

  for (const a of options.agentStatuses ?? []) {
    const wt = worktrees[a.worktreeIndex]!;
    upsertAgentStatus(wt.id, a.status, a.sessionId, a.lastResponse, a.transcriptSummary);
  }

  return { repos, worktrees };
}
