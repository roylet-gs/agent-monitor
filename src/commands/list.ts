import { resolve } from "path";
import { getRepositories, getRepositoryByPath } from "../lib/db.js";
import { loadSettings } from "../lib/settings.js";
import { enrichAllWorktrees } from "../lib/enrich.js";
import { getPrStatusLabel } from "../lib/github.js";
import type { WorktreeWithStatus } from "../lib/types.js";

export async function runList(flags: { repo?: string; json?: boolean }): Promise<void> {
  const settings = loadSettings();
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
    console.log("No repositories. Use: am repo add <path>");
    return;
  }

  const groups = await enrichAllWorktrees(repos, settings);

  if (flags.json) {
    console.log(JSON.stringify(groups, null, 2));
    return;
  }

  for (const group of groups) {
    if (groups.length > 1) {
      console.log(`\n${group.repo.name} (${group.repo.path})`);
      console.log("─".repeat(60));
    }

    if (group.worktrees.length === 0) {
      console.log("  (no worktrees)");
      continue;
    }

    for (const wt of group.worktrees) {
      console.log(formatWorktreeLine(wt));
    }
  }
}

function formatWorktreeLine(wt: WorktreeWithStatus): string {
  const parts: string[] = [];

  // Branch name (or custom name)
  const name = wt.custom_name ?? wt.branch;
  parts.push(name.padEnd(30));

  // Agent status
  const agent = wt.agent_status?.status ?? "—";
  parts.push(agent.padEnd(10));

  // Git status
  const git = wt.git_status;
  if (git) {
    const gitParts: string[] = [];
    if (git.dirty > 0) gitParts.push(`${git.dirty}M`);
    if (git.ahead > 0) gitParts.push(`↑${git.ahead}`);
    if (git.behind > 0) gitParts.push(`↓${git.behind}`);
    parts.push((gitParts.join(" ") || "clean").padEnd(12));
  } else {
    parts.push("".padEnd(12));
  }

  // PR status
  if (wt.pr_info) {
    const { label } = getPrStatusLabel(wt.pr_info);
    parts.push(`PR#${wt.pr_info.number} ${label}`.padEnd(25));
  } else {
    parts.push("".padEnd(25));
  }

  // Linear
  if (wt.linear_info) {
    parts.push(`${wt.linear_info.identifier} ${wt.linear_info.state.name}`);
  }

  // Last commit time
  if (wt.last_commit) {
    parts.push(`(${wt.last_commit.relative_time})`);
  }

  return parts.join("  ");
}
