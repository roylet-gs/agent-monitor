import { getManagedSessions, getAllWorktrees } from "../../lib/db.js";
import { isTurnRunning } from "../../lib/claude-session.js";
import { outputJson, outputTable } from "../../lib/output.js";

export function agentList(opts: { json?: boolean }): void {
  const sessions = getManagedSessions();
  const worktreesById = new Map(getAllWorktrees().map((w) => [w.id, w]));

  const rows = sessions.map((s) => {
    const wt = worktreesById.get(s.worktree_id);
    return {
      worktree: wt ? (wt.custom_name ?? wt.branch) : s.cwd,
      state: isTurnRunning(s) ? "running" : "idle",
      turns: String(s.turn_count),
      lastPrompt: s.last_prompt ?? "",
      session: s.id,
    };
  });

  if (opts.json) {
    outputJson(rows);
    return;
  }

  outputTable(rows, [
    { key: "worktree", header: "WORKTREE" },
    { key: "state", header: "STATE" },
    { key: "turns", header: "TURNS", align: "right" },
    { key: "lastPrompt", header: "LAST PROMPT", width: 40 },
    { key: "session", header: "SESSION" },
  ]);
}
