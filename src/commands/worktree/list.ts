import { existsSync } from "fs";
import { getWorktrees, getAgentStatuses, getRepositories } from "../../lib/db.js";
import { getGitStatus, getLastCommit } from "../../lib/git.js";
import { fetchPrInfo, getPrStatusLabel } from "../../lib/github.js";
import { fetchLinearInfo } from "../../lib/linear.js";
import { loadSettings } from "../../lib/settings.js";
import { resolveRepo } from "../../lib/resolve.js";
import { outputJson, outputTable, type TableColumn } from "../../lib/output.js";
import type { Repository } from "../../lib/types.js";

export async function worktreeList(opts: { repo?: string; json?: boolean }): Promise<void> {
  let repos: Repository[];
  if (opts.repo) {
    repos = [resolveRepo(opts.repo)];
  } else {
    // Try CWD detection, fall back to all repos
    const allRepos = getRepositories();
    if (allRepos.length === 0) {
      console.error("No repositories tracked. Run: am repo add <path>");
      process.exit(1);
    }
    repos = allRepos;
  }

  const settings = loadSettings();
  const allRows: Record<string, string>[] = [];
  const allJsonData: unknown[] = [];

  for (const repo of repos) {
    const worktrees = getWorktrees(repo.id);
    const statuses = getAgentStatuses(repo.id);

    for (const wt of worktrees) {
      if (settings.hideMainBranch && wt.branch === "main" || wt.branch === "master") {
        // Still include if explicitly listing one repo
        if (!opts.repo && repos.length > 1) continue;
      }

      if (!existsSync(wt.path)) continue;

      const agent = statuses.get(wt.id);
      const gitStatus = await getGitStatus(wt.path);
      const commit = await getLastCommit(wt.path);

      let prLabel = "";
      let prUrl = "";
      if (settings.ghPrStatus) {
        const pr = await fetchPrInfo(repo.path, wt.branch);
        if (pr) {
          const { label } = getPrStatusLabel(pr);
          prLabel = label;
          prUrl = pr.url;
        }
      }

      let linearLabel = "";
      if (settings.linearEnabled && settings.linearApiKey) {
        const linear = await fetchLinearInfo(wt.branch, settings.linearApiKey);
        if (linear) {
          linearLabel = `${linear.identifier} ${linear.state.name}`;
        }
      }

      const gitInfo = [
        gitStatus.dirty > 0 ? `${gitStatus.dirty}M` : "",
        gitStatus.ahead > 0 ? `↑${gitStatus.ahead}` : "",
        gitStatus.behind > 0 ? `↓${gitStatus.behind}` : "",
      ].filter(Boolean).join(" ");

      if (opts.json) {
        allJsonData.push({
          repo: repo.name,
          repoPath: repo.path,
          branch: wt.branch,
          path: wt.path,
          agent: agent?.status ?? "unknown",
          git: gitStatus,
          lastCommit: commit,
          pr: prLabel || null,
          prUrl: prUrl || null,
          linear: linearLabel || null,
        });
      } else {
        allRows.push({
          repo: repos.length > 1 ? repo.name : "",
          branch: wt.branch,
          agent: agent?.status ?? "—",
          git: gitInfo || "clean",
          commit: commit ? `${commit.hash} ${commit.relative_time}` : "",
          pr: prLabel,
          linear: linearLabel,
        });
      }
    }
  }

  if (opts.json) {
    outputJson(allJsonData);
    return;
  }

  const columns: TableColumn[] = [
    ...(repos.length > 1 ? [{ key: "repo", header: "Repo" }] : []),
    { key: "branch", header: "Branch" },
    { key: "agent", header: "Agent" },
    { key: "git", header: "Git" },
    { key: "commit", header: "Last Commit" },
    ...(allRows.some((r) => r.pr) ? [{ key: "pr", header: "PR" }] : []),
    ...(allRows.some((r) => r.linear) ? [{ key: "linear", header: "Linear" }] : []),
  ];

  outputTable(allRows, columns);
}
