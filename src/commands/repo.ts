import { resolve } from "path";
import { createInterface } from "readline";
import {
  getRepositories,
  addRepository,
  removeRepository,
  getRepositoryByPath,
} from "../lib/db.js";
import { isGitRepo, getRepoName } from "../lib/git.js";
import { syncWorktrees } from "../lib/sync.js";

export function runRepoList(flags: { json?: boolean }): void {
  const repos = getRepositories();
  if (flags.json) {
    console.log(JSON.stringify(repos, null, 2));
    return;
  }
  if (repos.length === 0) {
    console.log("No repositories added. Use: am repo add <path>");
    return;
  }
  for (const repo of repos) {
    console.log(`${repo.name}\t${repo.path}`);
  }
}

export async function runRepoAdd(path: string): Promise<void> {
  const absPath = resolve(path);
  if (!isGitRepo(absPath)) {
    console.error(`Error: ${absPath} is not a git repository`);
    process.exit(1);
  }
  const name = getRepoName(absPath);
  const repo = addRepository(absPath, name);
  console.log(`Added repository: ${repo.name} (${repo.path})`);
  await syncWorktrees(repo.id);
  console.log("Worktrees synced.");
}

export async function runRepoRemove(path: string, flags: { yes?: boolean }): Promise<void> {
  const absPath = resolve(path);
  const repo = getRepositoryByPath(absPath);
  if (!repo) {
    console.error(`Error: repository not found: ${absPath}`);
    process.exit(1);
  }

  if (!flags.yes) {
    const confirmed = await confirm(`Remove repository "${repo.name}" (${repo.path})? [y/N] `);
    if (!confirmed) {
      console.log("Cancelled.");
      return;
    }
  }

  removeRepository(repo.id);
  console.log(`Removed repository: ${repo.name}`);
}

function confirm(prompt: string): Promise<boolean> {
  const rl = createInterface({ input: process.stdin, output: process.stdout });
  return new Promise((res) => {
    rl.question(prompt, (answer) => {
      rl.close();
      res(answer.toLowerCase() === "y" || answer.toLowerCase() === "yes");
    });
  });
}
