// agent-monitor-hook-plugin v1
// agent-monitor (am) extension for OMP (oh-my-pi): reports this omp
// session's status to am by running am-hook with a JSON payload on stdin.
// Written by `am hooks install --agent omp`, removed by
// `am hooks uninstall --agent omp`. Don't edit: am rewrites it on install.
//
// Dependency-free on purpose (no import of OMP's package), never throws into
// OMP, and never waits on am-hook except briefly at session shutdown.
// Hooks run one at a time, in order (see `send`).

import { spawn } from "node:child_process";
import * as fs from "node:fs";
import * as path from "node:path";

// argv per event; `--worktree ""` makes am-hook use the payload's `cwd`.
const HOOKS: Record<string, string[]> = {
__AM_HOOKS__
};
const TIMEOUT_MS = __AM_TIMEOUT_MS__;
const MAX_TEXT = 32768;
const MAX_FIELD = 2000;

type Obj = Record<string, unknown>;
// OMP's ExtensionAPI / ExtensionContext, untyped so nothing imports OMP.
// biome-ignore lint/suspicious/noExplicitAny: see above
type Ctx = any;

// OMP loads a fresh copy of this module per session in the process (the main
// session and every task subagent), so what spans sessions lives on
// globalThis: the first top-level session seen owns the terminal, later ones
// are its subagents. `queue` is the tail of the hook runs, shared by every
// session so events from all of them reach am-hook in the order they fired.
interface State {
  owner: string | null;
  stopped: Record<string, boolean>;
  queue: Promise<void>;
}
const G = globalThis as typeof globalThis & { __agentMonitorOmp?: State };
function state(): State {
  if (!G.__agentMonitorOmp) G.__agentMonitorOmp = { owner: null, stopped: {}, queue: Promise.resolve() };
  if (!G.__agentMonitorOmp.queue) G.__agentMonitorOmp.queue = Promise.resolve();
  return G.__agentMonitorOmp;
}

// Tools whose name reads as "waiting on the user" (AskUserQuestion): their
// end is sent without the name, so it reads as working again, not waiting.
const WAITING_TOOLS = new Set(["ask", "plan_review"]);
function endedTool(name: unknown): unknown {
  return typeof name === "string" && WAITING_TOOLS.has(name) ? undefined : name;
}

function text(v: unknown): string | null {
  return typeof v === "string" && v.trim().length > 0 ? v.trim() : null;
}

function bounded(v: string | null | undefined): string | undefined {
  if (!v) return undefined;
  return v.length > MAX_TEXT ? v.slice(0, MAX_TEXT) : v;
}

// Tool input with long strings (a file's new contents) cut short.
function slim(v: unknown): Obj {
  if (!v || typeof v !== "object" || Array.isArray(v)) return {};
  const out: Obj = {};
  for (const [k, x] of Object.entries(v as Obj)) {
    out[k] = typeof x === "string" && x.length > MAX_FIELD ? x.slice(0, MAX_FIELD) : x;
  }
  return out;
}

function sessionId(ctx: Ctx): string | null {
  try {
    return text(ctx?.sessionManager?.getSessionId?.());
  } catch {
    return null;
  }
}

function cwdOf(ctx: Ctx): string {
  return text(ctx?.cwd) ?? process.cwd();
}

// A subagent's session file lives in a directory named after its parent's
// (`<parent>/<child>.jsonl` beside `<parent>.jsonl`).
function nested(ctx: Ctx): boolean {
  try {
    const file = text(ctx?.sessionManager?.getSessionFile?.());
    return file !== null && fs.existsSync(`${path.dirname(file)}.jsonl`);
  } catch {
    return false;
  }
}

// Pi reads PI_CODING_AGENT_DIR too, so with it set Pi may load this file:
// only report from inside omp.
function isOmp(ctx: Ctx): boolean {
  if (typeof ctx?.resolvePlanReview === "function") return true;
  const argv = [process.execPath, ...(Array.isArray(process.argv) ? process.argv.slice(0, 2) : [])].map(String);
  return argv.some((a) => /oh-my-pi/.test(a) || /^omp(\.exe)?$/i.test(path.basename(a)));
}

type Role = { sub: false; id: string } | { sub: true; id: string; owner: string };

function role(ctx: Ctx): Role | null {
  if (process.env.AM_OMP_HOOKS_DISABLED === "1" || !isOmp(ctx)) return null;
  const id = sessionId(ctx);
  if (!id) return null;
  const s = state();
  if (s.owner === null && !nested(ctx)) s.owner = id;
  if (id === s.owner) return { sub: false, id };
  if (s.owner === null) return null;
  return { sub: true, id, owner: s.owner };
}

function payload(r: Role, ctx: Ctx, extra: Obj = {}): Obj {
  const p: Obj = { session_id: r.sub ? r.owner : r.id, cwd: cwdOf(ctx), ...extra };
  if (r.sub) {
    p.agent_id = r.id;
    p.agent_type = "task";
  }
  return p;
}

// Queue one am-hook run after the ones before it: OMP fires events
// microseconds apart, and hook processes racing to commit would land them
// out of order (a PreToolUse after its PermissionRequest, a PostToolUse after
// the Stop). Returns the queue's new tail; callers don't await it, except at
// shutdown. Never rejects.
function send(event: string, p: Obj): Promise<void> {
  const argv = HOOKS[event];
  if (!argv) return Promise.resolve();
  const s = state();
  const next = s.queue.then(() => run(argv, event, p), () => run(argv, event, p));
  s.queue = next;
  return next;
}

// Run am-hook detached with the payload on stdin. Resolves when it exits (or
// on any failure); one still running after TIMEOUT_MS is killed.
function run(argv: string[], event: string, p: Obj): Promise<void> {
  return new Promise<void>((resolve) => {
    let settled = false;
    const done = () => {
      if (!settled) {
        settled = true;
        resolve();
      }
    };
    try {
      const child = spawn(argv[0], argv.slice(1), { detached: true, stdio: ["pipe", "ignore", "ignore"] });
      const timer = setTimeout(() => {
        try {
          child.kill("SIGKILL");
        } catch {}
        done();
      }, TIMEOUT_MS);
      timer.unref?.();
      const finish = () => {
        clearTimeout(timer);
        done();
      };
      child.on("error", finish);
      child.on("exit", finish);
      child.stdin?.on("error", () => {});
      child.stdin?.end(JSON.stringify({ ...p, hook_event_name: event }));
      child.unref();
    } catch {
      done();
    }
  });
}

function settleWithin(work: Promise<void>, ms: number): Promise<void> {
  return new Promise<void>((resolve) => {
    const timer = setTimeout(resolve, ms);
    timer.unref?.();
    work.then(() => {
      clearTimeout(timer);
      resolve();
    });
  });
}

function lastAssistant(messages: unknown): string | undefined {
  if (!Array.isArray(messages)) return undefined;
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    const m = messages[i] as { role?: unknown; content?: unknown } | null;
    if (!m || typeof m !== "object" || m.role !== "assistant") continue;
    if (typeof m.content === "string" && m.content.trim()) return bounded(m.content.trim());
    if (!Array.isArray(m.content)) continue;
    const parts = m.content
      .filter((b: { type?: unknown; text?: unknown }) => b && b.type === "text" && typeof b.text === "string")
      .map((b: { text: string }) => b.text);
    const joined = parts.join("\n").trim();
    if (joined) return bounded(joined);
  }
  return undefined;
}

export default function agentMonitor(api: Ctx) {
  const on = (name: string, handler: (event: Obj, ctx: Ctx) => unknown) => {
    try {
      api.on(name, (event: Obj, ctx: Ctx) => {
        try {
          const out = handler(event ?? {}, ctx);
          return out instanceof Promise ? out.then(() => undefined, () => undefined) : undefined;
        } catch {
          return undefined;
        }
      });
    } catch {}
  };

  on("session_start", (_e, ctx) => {
    const r = role(ctx);
    if (r) void send(r.sub ? "subagent_start" : "session_start", payload(r, ctx));
  });

  // /new, resume, fork, handoff: the terminal's session changes. OMP's
  // reload re-emits session_switch for the same session; that's no change.
  const switched = (_e: Obj, ctx: Ctx) => {
    if (process.env.AM_OMP_HOOKS_DISABLED === "1" || !isOmp(ctx) || nested(ctx)) return;
    const id = sessionId(ctx);
    const s = state();
    if (!id || id === s.owner) return;
    const old = s.owner;
    s.owner = id;
    const cwd = cwdOf(ctx);
    if (old) void send("session_shutdown", { session_id: old, cwd });
    void send("session_start", { session_id: id, cwd });
  };
  on("session_switch", switched);
  on("session_branch", switched);

  on("before_agent_start", (e, ctx) => {
    const r = role(ctx);
    if (!r) return;
    if (r.sub) {
      // A subagent's turn (also a parked one revived).
      void send("subagent_start", payload(r, ctx));
      return;
    }
    state().stopped[r.id] = false;
    void send("before_agent_start", payload(r, ctx, { prompt: bounded(text(e.prompt)) }));
  });

  on("tool_execution_start", (e, ctx) => {
    const r = role(ctx);
    if (!r) return;
    if (!r.sub) state().stopped[r.id] = false;
    void send("tool_execution_start", payload(r, ctx, { tool_name: e.toolName, tool_input: slim(e.args) }));
  });

  on("tool_execution_end", (e, ctx) => {
    const r = role(ctx);
    if (r) void send("tool_execution_end", payload(r, ctx, { tool_name: endedTool(e.toolName) }));
  });

  // An approval prompt is on screen (from the session or one of its subagents).
  on("tool_approval_requested", (e, ctx) => {
    const r = role(ctx);
    if (r) void send("tool_approval_requested", payload(r, ctx, { tool_name: e.toolName, message: text(e.reason) ?? undefined }));
  });

  on("tool_approval_resolved", (e, ctx) => {
    const r = role(ctx);
    if (r) void send("tool_approval_resolved", payload(r, ctx, { tool_name: endedTool(e.toolName) }));
  });

  // The plan review is shown after the turn ends (as a tool that waits on the
  // user), so it reads as Claude's ExitPlanMode.
  on("plan_review_requested", (e, ctx) => {
    const r = role(ctx);
    if (r && !r.sub) void send("plan_review_requested", payload(r, ctx, { tool_name: "plan_review", tool_input: { title: text(e.title) ?? undefined } }));
  });

  on("agent_end", (e, ctx) => {
    // willContinue: an automatic continuation follows (retry, queued
    // message, background job), so the turn isn't over.
    if (e.willContinue === true) return;
    const r = role(ctx);
    if (!r) return;
    const last = lastAssistant(e.messages);
    if (r.sub) {
      void send("subagent_stop", payload(r, ctx, { last_assistant_message: last }));
      return;
    }
    // One Stop per turn: a second one would turn done into idle.
    const s = state();
    if (s.stopped[r.id]) return;
    s.stopped[r.id] = true;
    void send("agent_end", payload(r, ctx, { last_assistant_message: last }));
  });

  on("session_shutdown", (_e, ctx) => {
    const r = role(ctx);
    if (!r) return;
    if (r.sub) {
      void send("subagent_stop", payload(r, ctx));
      return;
    }
    const s = state();
    if (s.owner === r.id) s.owner = null;
    delete s.stopped[r.id];
    // omp may exit right after this: let the queued hooks (this one last)
    // finish first, for up to a second.
    return settleWithin(send("session_shutdown", payload(r, ctx)), 1000);
  });
}
