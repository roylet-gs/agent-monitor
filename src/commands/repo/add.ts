import { resolve } from "path";
import { isGitRepo, getRepoName } from "../../lib/git.js";
import { addRepository, getRepositoryByPath } from "../../lib/db.js";
import { syncWorktrees } from "../../lib/sync.js";
import { outputJson } from "../../lib/output.js";

export async function repoAdd(path: string, opts: { json?: boolean }): Promise<void> {
  const absPath = resolve(path);

  if (!isGitRepo(absPath)) {
    console.error(`Not a git repository: ${absPath}`);
    process.exit(1);
  }

  const existing = getRepositoryByPath(absPath);
  if (existing) {
    console.log(`Repository already tracked: ${existing.name} (${existing.path})`);
    return;
  }

  const name = getRepoName(absPath);
  const repo = addRepository(absPath, name);
  await syncWorktrees(repo.id);

  if (opts.json) {
    outputJson(repo);
  } else {
    console.log(`Added repository: ${repo.name} (${repo.path})`);
  }
}
