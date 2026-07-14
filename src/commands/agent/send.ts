import { resolveWorktree, resolveRepo } from "../../lib/resolve.js";
import { loadSettings } from "../../lib/settings.js";
import { startTurn, isTurnRunning, parseTranscript } from "../../lib/claude-session.js";
import { getManagedSessionById } from "../../lib/db.js";
import { outputJson } from "../../lib/output.js";

export async function agentSend(
  target: string,
  promptWords: string[],
  opts: { repo?: string; wait?: boolean; json?: boolean; session?: string }
): Promise<void> {
  const prompt = promptWords.join(" ").trim();
  if (!prompt) {
    console.error("Empty prompt.");
    process.exit(1);
  }

  const repoId = opts.repo ? resolveRepo(opts.repo).id : undefined;
  const worktree = resolveWorktree(target, repoId);
  const settings = loadSettings();

  let sessionId: string | undefined;
  if (opts.session) {
    const { resolveSessionId } = await import("./sessions.js");
    sessionId = resolveSessionId(worktree.path, opts.session).id;
  }

  let session;
  try {
    session = startTurn(worktree, prompt, settings, sessionId);
  } catch (err) {
    console.error(`${err instanceof Error ? err.message : err}`);
    process.exit(1);
  }

  if (!opts.wait) {
    if (opts.json) {
      outputJson({ sessionId: session.id, worktree: worktree.branch, pid: session.turn_pid, turn: session.turn_count });
    } else {
      console.log(`Prompt sent to ${worktree.custom_name ?? worktree.branch} (session ${session.id}).`);
      console.log(`View: am agent log ${target}   Attach: am agent attach ${target}`);
    }
    return;
  }

  // --wait: poll until the detached turn process exits, then print the outcome
  while (true) {
    const current = getManagedSessionById(session.id);
    if (!current || !isTurnRunning(current)) break;
    await new Promise((r) => setTimeout(r, 500));
  }

  const transcript = parseTranscript(session.id);
  const lastError = [...transcript].reverse().find((m) => m.role === "error");
  const lastAssistant = [...transcript].reverse().find((m) => m.role === "assistant");

  if (opts.json) {
    outputJson({
      sessionId: session.id,
      worktree: worktree.branch,
      ok: !lastError,
      response: lastAssistant?.text ?? null,
      error: lastError?.text ?? null,
    });
  } else if (lastError) {
    console.error(lastError.text);
    process.exit(1);
  } else {
    console.log(lastAssistant?.text ?? "(no response)");
  }
}
