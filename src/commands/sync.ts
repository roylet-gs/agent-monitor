import { resolve } from "path";
import { getRepositories, getRepositoryByPath } from "../lib/db.js";
import { syncWorktrees } from "../lib/sync.js";

export async function runSync(flags: { repo?: string }): Promise<void> {
  let repos = getRepositories();

  if (flags.repo) {
    const absPath = resolve(flags.repo);
    const repo = getRepositoryByPath(absPath);
    if (!repo) {
      console.error(`Error: repository not found: ${absPath}`);
      process.exit(1);
    }
    repos = [repo];
  }

  if (repos.length === 0) {
    console.log("No repositories to sync. Use: am repo add <path>");
    return;
  }

  for (const repo of repos) {
    await syncWorktrees(repo.id);
    console.log(`Synced: ${repo.name}`);
  }
}
