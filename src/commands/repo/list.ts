import { getRepositories, getWorktrees } from "../../lib/db.js";
import { outputJson, outputTable } from "../../lib/output.js";

export function repoList(opts: { json?: boolean }): void {
  const repos = getRepositories();

  if (opts.json) {
    outputJson(repos.map((r) => ({
      ...r,
      worktreeCount: getWorktrees(r.id).length,
    })));
    return;
  }

  if (repos.length === 0) {
    console.log("No repositories tracked. Run: am repo add <path>");
    return;
  }

  outputTable(
    repos.map((r) => ({
      name: r.name,
      path: r.path,
      worktrees: String(getWorktrees(r.id).length),
      lastUsed: r.last_used_at,
    })),
    [
      { key: "name", header: "Name" },
      { key: "path", header: "Path" },
      { key: "worktrees", header: "Worktrees", align: "right" },
      { key: "lastUsed", header: "Last Used" },
    ]
  );
}
