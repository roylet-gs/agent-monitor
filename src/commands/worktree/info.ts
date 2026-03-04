import { existsSync } from "fs";
import { getAgentStatus, getRepositoryById } from "../../lib/db.js";
import { getGitStatus, getLastCommit } from "../../lib/git.js";
import { fetchPrInfo, getPrStatusLabel } from "../../lib/github.js";
import { fetchLinearInfo } from "../../lib/linear.js";
import { loadSettings } from "../../lib/settings.js";
import { resolveWorktree, resolveRepo } from "../../lib/resolve.js";
import { outputJson, outputKeyValue } from "../../lib/output.js";

export async function worktreeInfo(target: string, opts: { repo?: string; json?: boolean }): Promise<void> {
  const repo = resolveRepo(opts.repo);
  const worktree = resolveWorktree(target, repo.id);
  const settings = loadSettings();

  const agent = getAgentStatus(worktree.id);
  const pathExists = existsSync(worktree.path);
  const gitStatus = pathExists ? await getGitStatus(worktree.path) : null;
  const commit = pathExists ? await getLastCommit(worktree.path) : null;

  let prInfo = null;
  if (settings.ghPrStatus) {
    prInfo = await fetchPrInfo(repo.path, worktree.branch);
  }

  let linearInfo = null;
  if (settings.linearEnabled && settings.linearApiKey) {
    linearInfo = await fetchLinearInfo(worktree.branch, settings.linearApiKey);
  }

  if (opts.json) {
    outputJson({
      branch: worktree.branch,
      path: worktree.path,
      exists: pathExists,
      repo: repo.name,
      repoPath: repo.path,
      agent: agent ? { status: agent.status, sessionId: agent.session_id, updatedAt: agent.updated_at } : null,
      git: gitStatus,
      lastCommit: commit,
      pr: prInfo,
      linear: linearInfo,
    });
    return;
  }

  const pairs: [string, string][] = [
    ["Branch:", worktree.branch],
    ["Path:", worktree.path],
    ["Exists:", pathExists ? "yes" : "NO — worktree directory missing"],
    ["Repo:", `${repo.name} (${repo.path})`],
    ["Agent:", agent?.status ?? "unknown"],
  ];

  if (agent?.session_id) {
    pairs.push(["Session:", agent.session_id]);
  }
  if (agent?.updated_at) {
    pairs.push(["Updated:", agent.updated_at]);
  }

  if (gitStatus) {
    const parts = [
      gitStatus.dirty > 0 ? `${gitStatus.dirty} modified` : "clean",
      gitStatus.ahead > 0 ? `${gitStatus.ahead} ahead` : "",
      gitStatus.behind > 0 ? `${gitStatus.behind} behind` : "",
    ].filter(Boolean).join(", ");
    pairs.push(["Git:", parts]);
  }

  if (commit) {
    pairs.push(["Commit:", `${commit.hash} ${commit.message} (${commit.relative_time})`]);
  }

  if (prInfo) {
    const { label } = getPrStatusLabel(prInfo);
    pairs.push(["PR:", `#${prInfo.number} ${prInfo.title}`]);
    pairs.push(["PR Status:", label]);
    pairs.push(["PR URL:", prInfo.url]);
  }

  if (linearInfo) {
    pairs.push(["Linear:", `${linearInfo.identifier} ${linearInfo.title}`]);
    pairs.push(["Linear Status:", linearInfo.state.name]);
    pairs.push(["Linear URL:", linearInfo.url]);
  }

  if (agent?.transcript_summary) {
    pairs.push(["Summary:", agent.transcript_summary.slice(0, 200)]);
  }

  outputKeyValue(pairs);
}
