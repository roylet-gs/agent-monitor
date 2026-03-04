import openUrl from "open";
import { fetchPrInfo, getPrStatusLabel } from "../lib/github.js";
import { getRepositoryById } from "../lib/db.js";
import { resolveWorktree, resolveRepo, detectRepo } from "../lib/resolve.js";
import { outputJson, outputKeyValue } from "../lib/output.js";

async function getPrForTarget(target?: string, repoPath?: string) {
  const repo = resolveRepo(repoPath);
  let branch: string;

  if (target) {
    const wt = resolveWorktree(target, repo.id);
    branch = wt.branch;
  } else {
    // Use current branch from CWD
    const { getGit } = await import("../lib/git.js");
    const git = getGit(process.cwd());
    branch = (await git.raw(["rev-parse", "--abbrev-ref", "HEAD"])).trim();
  }

  const pr = await fetchPrInfo(repo.path, branch);
  return { pr, branch, repo };
}

export async function prShow(target?: string, opts: { repo?: string; json?: boolean } = {}): Promise<void> {
  const { pr, branch } = await getPrForTarget(target, opts.repo);

  if (!pr) {
    console.log(`No PR found for branch: ${branch}`);
    return;
  }

  if (opts.json) {
    outputJson(pr);
    return;
  }

  const { label } = getPrStatusLabel(pr);
  outputKeyValue([
    ["PR:", `#${pr.number} ${pr.title}`],
    ["Status:", label],
    ["State:", pr.state],
    ["Draft:", pr.isDraft ? "yes" : "no"],
    ["Checks:", pr.checksStatus],
    ["URL:", pr.url],
  ]);
}

export async function prOpen(target?: string, opts: { repo?: string } = {}): Promise<void> {
  const { pr, branch } = await getPrForTarget(target, opts.repo);

  if (!pr) {
    console.log(`No PR found for branch: ${branch}`);
    return;
  }

  await openUrl(pr.url);
  console.log(`Opened: ${pr.url}`);
}
