import open from "open";
import { getRepositoryById } from "../lib/db.js";
import { fetchPrInfo } from "../lib/github.js";
import { resolveWorktree, type ResolveOptions } from "./_resolve.js";

export async function runOpenPr(opts: ResolveOptions): Promise<void> {
  const wt = resolveWorktree(opts);
  const repo = getRepositoryById(wt.repo_id);
  if (!repo) {
    console.error("Error: repository not found for worktree");
    process.exit(1);
  }

  const pr = await fetchPrInfo(repo.path, wt.branch);
  if (!pr) {
    console.error(`No PR found for branch "${wt.branch}"`);
    process.exit(1);
  }

  await open(pr.url);
  console.log(`Opened PR #${pr.number}: ${pr.url}`);
}
