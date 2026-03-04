import { resolve } from "path";
import { getRepositories, getRepositoryByPath, removeRepository } from "../../lib/db.js";

export function repoRemove(nameOrPath: string): void {
  // Try as path first
  const absPath = resolve(nameOrPath);
  let repo = getRepositoryByPath(absPath);

  // Try as name
  if (!repo) {
    const repos = getRepositories();
    repo = repos.find((r) => r.name === nameOrPath);
  }

  if (!repo) {
    console.error(`Repository not found: ${nameOrPath}`);
    process.exit(1);
  }

  removeRepository(repo.id);
  console.log(`Removed repository: ${repo.name} (${repo.path})`);
}
