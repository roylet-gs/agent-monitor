import { getRepositories } from "../../lib/db.js";
import { syncWorktrees } from "../../lib/sync.js";
import { resolveRepo, detectRepo } from "../../lib/resolve.js";
import type { Repository } from "../../lib/types.js";

export async function worktreeSync(opts: { repo?: string }): Promise<void> {
  let repos: Repository[];

  if (opts.repo) {
    repos = [resolveRepo(opts.repo)];
  } else {
    const detected = detectRepo();
    repos = detected ? [detected] : getRepositories();
  }

  if (repos.length === 0) {
    console.error("No repositories tracked. Run: am repo add <path>");
    process.exit(1);
  }

  for (const repo of repos) {
    await syncWorktrees(repo.id);
    console.log(`Synced: ${repo.name} (${repo.path})`);
  }
}
