import { getAgentStatuses, getRepositoryById } from "../lib/db.js";
import { loadSettings } from "../lib/settings.js";
import { enrichWorktree } from "../lib/enrich.js";
import { getPrStatusLabel } from "../lib/github.js";
import { resolveWorktree, type ResolveOptions } from "./_resolve.js";

export async function runStatus(opts: ResolveOptions, flags: { json?: boolean }): Promise<void> {
  const wt = resolveWorktree(opts);
  const repo = getRepositoryById(wt.repo_id);
  if (!repo) {
    console.error(`Error: repository not found for worktree`);
    process.exit(1);
  }

  const settings = loadSettings();
  const statuses = getAgentStatuses(wt.repo_id);
  const enriched = await enrichWorktree(wt, statuses, repo.path, {
    ghPrStatus: settings.ghPrStatus,
    linearEnabled: settings.linearEnabled,
    linearApiKey: settings.linearApiKey,
  });

  if (flags.json) {
    console.log(JSON.stringify(enriched, null, 2));
    return;
  }

  console.log(`Worktree:  ${enriched.path}`);
  console.log(`Branch:    ${enriched.branch}`);
  if (enriched.custom_name) {
    console.log(`Name:      ${enriched.custom_name}`);
  }
  console.log(`Repo:      ${repo.name} (${repo.path})`);

  // Agent status
  const agent = enriched.agent_status;
  console.log(`Agent:     ${agent?.status ?? "unknown"}`);
  if (agent?.session_id) {
    console.log(`Session:   ${agent.session_id}`);
  }
  if (agent?.updated_at) {
    console.log(`Updated:   ${agent.updated_at}`);
  }
  if (agent?.transcript_summary) {
    console.log(`Summary:   ${agent.transcript_summary}`);
  }
  if (agent?.last_response) {
    const truncated =
      agent.last_response.length > 200
        ? agent.last_response.slice(0, 200) + "..."
        : agent.last_response;
    console.log(`Response:  ${truncated}`);
  }

  // Git status
  const git = enriched.git_status;
  if (git) {
    const parts: string[] = [];
    if (git.dirty > 0) parts.push(`${git.dirty} modified`);
    if (git.ahead > 0) parts.push(`${git.ahead} ahead`);
    if (git.behind > 0) parts.push(`${git.behind} behind`);
    console.log(`Git:       ${parts.length > 0 ? parts.join(", ") : "clean"}`);
  }

  // Last commit
  if (enriched.last_commit) {
    console.log(`Commit:    ${enriched.last_commit.hash} ${enriched.last_commit.message} (${enriched.last_commit.relative_time})`);
  }

  // PR info
  if (enriched.pr_info) {
    const { label } = getPrStatusLabel(enriched.pr_info);
    console.log(`PR:        #${enriched.pr_info.number} ${enriched.pr_info.title} [${label}]`);
    console.log(`PR URL:    ${enriched.pr_info.url}`);
  }

  // Linear info
  if (enriched.linear_info) {
    console.log(`Linear:    ${enriched.linear_info.identifier} ${enriched.linear_info.title} [${enriched.linear_info.state.name}]`);
    console.log(`Linear URL: ${enriched.linear_info.url}`);
  }
}

/**
 * @deprecated Use runStatus instead. Kept for backwards compatibility during migration.
 */
export function printStatus(worktreePath?: string): void {
  runStatus({ worktree: worktreePath }, {}).catch((err) => {
    console.error("Error:", err);
    process.exit(1);
  });
}
