import openUrl from "open";
import { fetchLinearInfo } from "../lib/linear.js";
import { loadSettings } from "../lib/settings.js";
import { resolveWorktree, resolveRepo } from "../lib/resolve.js";
import { outputJson, outputKeyValue } from "../lib/output.js";

async function getLinearForTarget(target?: string, repoPath?: string) {
  const settings = loadSettings();
  if (!settings.linearEnabled || !settings.linearApiKey) {
    console.error("Linear integration not configured. Set linearEnabled=true and linearApiKey in settings.");
    process.exit(1);
  }

  const repo = resolveRepo(repoPath);
  let branch: string;

  if (target) {
    const wt = resolveWorktree(target, repo.id);
    branch = wt.branch;
  } else {
    const { getGit } = await import("../lib/git.js");
    const git = getGit(process.cwd());
    branch = (await git.raw(["rev-parse", "--abbrev-ref", "HEAD"])).trim();
  }

  const info = await fetchLinearInfo(branch, settings.linearApiKey);
  return { info, branch };
}

export async function linearShow(target?: string, opts: { repo?: string; json?: boolean } = {}): Promise<void> {
  const { info, branch } = await getLinearForTarget(target, opts.repo);

  if (!info) {
    console.log(`No Linear ticket found for branch: ${branch}`);
    return;
  }

  if (opts.json) {
    outputJson(info);
    return;
  }

  outputKeyValue([
    ["Ticket:", `${info.identifier} ${info.title}`],
    ["Status:", info.state.name],
    ["Priority:", info.priorityLabel],
    ["Assignee:", info.assignee ?? "unassigned"],
    ["URL:", info.url],
  ]);
}

export async function linearOpen(target?: string, opts: { repo?: string } = {}): Promise<void> {
  const settings = loadSettings();
  const { info, branch } = await getLinearForTarget(target, opts.repo);

  if (!info) {
    console.log(`No Linear ticket found for branch: ${branch}`);
    return;
  }

  const url = settings.linearUseDesktopApp
    ? info.url.replace("https://linear.app", "linear://")
    : info.url;

  await openUrl(url);
  console.log(`Opened: ${url}`);
}
