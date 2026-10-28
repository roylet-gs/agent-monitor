//! Pi (Earendil's `pi` coding agent, npm `@earendil-works/pi-coding-agent`,
//! formerly `@mariozechner/pi-coding-agent`) as a built-in plugin: the
//! manifest `pi.json`, a generated extension, and session discovery.
//!
//! Pi has no shell hooks. It loads TypeScript/JavaScript extensions from
//! `~/.pi/agent/extensions/` (`$PI_CODING_AGENT_DIR/extensions/` when set), so
//! `am hooks install --agent pi` writes an am-owned `agent-monitor.ts` there
//! ([`HookInstall::OwnedFile`]). It is plain JavaScript (valid TypeScript)
//! importing only Node built-ins, so Pi loads it without resolving any
//! package. It subscribes to Pi's events and runs am-hook once per event
//! (`[am-hook, "hook-event", "--agent", "pi", "--worktree", "", "--event", E]`,
//! no shell, a Claude-shaped JSON payload on stdin: `session_id`, `cwd`,
//! `hook_event_name`, `tool_name`, `tool_input`, `agent_id`…). Runs are queued
//! one at a time so am sees them in order, never block or throw into Pi, and
//! are killed after 5 s; tool events are dropped when 32 runs are queued.
//!
//! Event mapping (native names are the canonical ones):
//! - `session_start` → `SessionStart`; `session_shutdown` → `SessionEnd`.
//! - `before_agent_start` → `UserPromptSubmit`; `agent_start` too, but only
//!   when no turn is open (a run started by `sendMessage({triggerTurn})`).
//! - `tool_execution_start`/`_end` → `PreToolUse`/`PostToolUse` with Pi's
//!   tool names (`bash`, `read`… mapped onto Claude's in the manifest) and a
//!   small `tool_input` (`path` → `file_path`). A subagent tool (`subagent`,
//!   `task`, `agent`, `team_spawn`, or any name containing "subagent") is
//!   `SubagentStart`/`SubagentStop` with the tool call id as `agent_id`.
//!   Calls another tool made (`parentToolCallId`) are skipped.
//! - `Stop`, once per turn: Pi ≥ 0.80.5 emits `agent_settled` once no retry,
//!   compaction or queued message will continue the run, so Stop waits for it
//!   (and `ctx.isIdle()`); older versions settle on `agent_end`. The version
//!   is read from the `package.json` above `process.argv[1]`; unknown counts
//!   as old. A second Stop would flip `done` to `idle`.
//! - Waiting: Pi has no permission event, so (like cmux) the extension wraps
//!   `ctx.ui.confirm`/`select`/`input`: while a turn is open a dialog sends
//!   `PermissionRequest` (waiting), and its answer `PostToolUse` (executing).
//!
//! Sessions: `<sessions dir>/--<cwd, "/" → "-">--/<time>_<id>.jsonl`, whose
//! first line is `{"type":"session","id","cwd",…}`; the sessions dir is
//! `$PI_CODING_AGENT_SESSION_DIR` (flat, every cwd in one dir) or
//! `<agent dir>/sessions`. Pi's `sessionDir` setting isn't read. Resume is
//! `pi --session <id>`.
//!
//! Presence (`configDir`) is `~/.pi`, so with only `$PI_CODING_AGENT_DIR` Pi
//! isn't detected; `am hooks install --agent pi` still installs there.

use std::ffi::OsString;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::Value;

use super::embedded::Embedded;
use super::manifest::Manifest;
use super::{HarnessKind, is_within};
use crate::hooks_installer::{HookContext, HookInstall, OWNED_FILE_MARKER, OwnedFile, js_string};
use crate::launch::Session;

pub static PI: Embedded = Embedded::new(|| Manifest::builtin(include_str!("pi.json")), Some(discover))
    .with_hook_file(hook_file)
    .with_hook_install(|| HookInstall::OwnedFile(OwnedFile { render, register: None }));

/// The extension's file name in Pi's `extensions/` dir.
pub const EXTENSION_FILE: &str = "agent-monitor.ts";

fn hook_file() -> (PathBuf, &'static str) {
    (agent_dir_from(std::env::var_os("PI_CODING_AGENT_DIR"), &crate::paths::home_dir()).join("extensions").join(EXTENSION_FILE), "")
}

/// Pi's agent dir: `$PI_CODING_AGENT_DIR` (a leading `~` expanded, as Pi
/// does) or `~/.pi/agent`.
fn agent_dir_from(env: Option<OsString>, home: &Path) -> PathBuf {
    match env.filter(|v| !v.is_empty()).map(PathBuf::from) {
        Some(dir) => match dir.strip_prefix("~") {
            Ok(rest) => home.join(rest),
            Err(_) => dir,
        },
        None => home.join(".pi/agent"),
    }
}

fn discover(root: &Path) -> Vec<Session> {
    let home = crate::paths::home_dir();
    let dir = match std::env::var_os("PI_CODING_AGENT_SESSION_DIR").filter(|v| !v.is_empty()) {
        Some(d) => agent_dir_from(Some(d), &home),
        None => agent_dir_from(std::env::var_os("PI_CODING_AGENT_DIR"), &home).join("sessions"),
    };
    discover_sessions_in(&dir, root)
}

/// Sessions started at `root` or below, newest first. Per-cwd subdirectories
/// (`--Users-me-repo--`) are only opened when their name could hold `root`;
/// session files directly in `dir` (a custom sessions dir) are all checked.
pub fn discover_sessions_in(dir: &Path, root: &Path) -> Vec<Session> {
    let root_s = root.to_string_lossy();
    let prefix = format!("--{}", root_s.trim_start_matches('/').replace(['/', '\\', ':'], "-"));
    let mut files = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = e.path();
        if path.is_dir() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with(&prefix) {
                files.extend(std::fs::read_dir(&path).into_iter().flatten().flatten().map(|f| f.path()));
            }
        } else {
            files.push(path);
        }
    }
    let mut sessions: Vec<Session> = files
        .into_iter()
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .filter_map(|p| {
            let (id, cwd) = session_header(&p)?;
            if !is_within(&cwd, &root_s) {
                return None;
            }
            let modified = p.metadata().and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
            Some(Session { id, cwd, modified, agent: HarnessKind::PI })
        })
        .collect();
    sessions.sort_by_key(|s| std::cmp::Reverse(s.modified));
    sessions
}

/// The session id and cwd from a session file's header line.
fn session_header(file: &Path) -> Option<(String, String)> {
    let mut first = String::new();
    BufReader::new(std::fs::File::open(file).ok()?).read_line(&mut first).ok()?;
    let line: Value = serde_json::from_str(first.trim()).ok()?;
    if line.get("type").and_then(Value::as_str) != Some("session") {
        return None;
    }
    let id = line.get("id")?.as_str().filter(|s| !s.is_empty())?.to_owned();
    let cwd = line.get("cwd")?.as_str()?.to_owned();
    Some((id, cwd))
}

/// The extension's text. `ARGV` is [`HookContext::argv`] without the event,
/// which the extension appends per run.
pub fn render(ctx: &HookContext) -> String {
    let mut argv = ctx.argv("", "");
    argv.pop();
    let argv = format!("[{}]", argv.iter().map(|a| js_string(a)).collect::<Vec<_>>().join(", "));
    let timeout = ctx.hooks.first().map(|h| h.timeout_ms).unwrap_or(5_000);
    EXTENSION
        .replace("__MARKER__", OWNED_FILE_MARKER)
        .replace("__ARGV__", &argv)
        .replace("__TIMEOUT_MS__", &timeout.to_string())
}

const EXTENSION: &str = r#"// __MARKER__: generated by agent-monitor (am) for Pi.
// `am hooks install --agent pi` rewrites this file and `am hooks uninstall --agent pi` deletes it.
// It reports Pi's session, turn and tool events to am-hook. Plain JavaScript importing only Node
// built-ins, so Pi loads it without resolving any package.
import { spawn } from "node:child_process";
import { readFileSync, realpathSync } from "node:fs";
import { dirname, join } from "node:path";

const ARGV = __ARGV__;
const TIMEOUT_MS = __TIMEOUT_MS__;
const MAX_QUEUED = 32;
const SUBAGENT_TOOLS = new Set(["subagent", "task", "agent", "team_spawn"]);

let queue = Promise.resolve();
let queued = 0;

// One am-hook run, payload on stdin; resolves on exit, failure or timeout and never throws. The
// child stays referenced so a run queued at quit (SessionEnd) finishes before Node exits; it is
// detached so Ctrl-C in Pi's terminal doesn't reach it.
function run(event, payload) {
  return new Promise((resolve) => {
    let timer;
    const done = () => { clearTimeout(timer); resolve(); };
    try {
      const child = spawn(ARGV[0], [...ARGV.slice(1), event], { stdio: ["pipe", "ignore", "ignore"], detached: true });
      timer = setTimeout(() => { try { child.kill("SIGKILL"); } catch {} done(); }, TIMEOUT_MS);
      timer.unref?.();
      child.on("error", done);
      child.on("close", done);
      child.stdin.on("error", () => {});
      child.stdin.end(JSON.stringify(payload));
    } catch {
      done();
    }
  });
}

function sessionId(ctx) {
  try { return ctx.sessionManager.getSessionId() || ""; } catch { return ""; }
}

// Queue a hook so am sees events in order. Tool events are dropped while many runs are queued.
function send(ctx, event, extra = {}, droppable = false) {
  const id = sessionId(ctx);
  if (!id || (droppable && queued >= MAX_QUEUED)) return queue;
  let cwd = process.cwd();
  try { cwd = ctx.cwd || cwd; } catch {}
  const payload = { session_id: id, cwd, hook_event_name: event, ...extra };
  queued++;
  queue = queue.then(() => run(event, payload)).catch(() => {}).finally(() => { queued--; });
  return queue;
}

// The few tool arguments am summarises, with Claude's key names.
function toolInput(args) {
  const out = {};
  if (!args || typeof args !== "object") return out;
  for (const [from, to] of [["command", "command"], ["description", "description"], ["task", "description"], ["path", "file_path"], ["file_path", "file_path"], ["pattern", "pattern"], ["url", "url"], ["query", "query"]]) {
    const v = args[from];
    if (typeof v === "string" && !(to in out)) out[to] = v.slice(0, 500);
  }
  return out;
}

function isSubagentTool(name) {
  return typeof name === "string" && (SUBAGENT_TOOLS.has(name.toLowerCase()) || /subagent/i.test(name));
}

// Pi >= 0.80.5 emits agent_settled once nothing will continue the run.
let settledKnown;
function emitsSettled() {
  if (settledKnown !== undefined) return settledKnown;
  settledKnown = false;
  try {
    let dir = dirname(realpathSync(process.argv[1]));
    for (let i = 0; i < 8; i++) {
      try {
        const pkg = JSON.parse(readFileSync(join(dir, "package.json"), "utf8"));
        if (/pi-coding-agent$/.test(String(pkg.name))) {
          const m = /^(\d+)\.(\d+)\.(\d+)/.exec(String(pkg.version)) || [];
          settledKnown = +m[1] > 0 || +m[2] > 80 || (+m[2] === 80 && +m[3] >= 5);
          break;
        }
      } catch {}
      const up = dirname(dir);
      if (up === dir) break;
      dir = up;
    }
  } catch {}
  return settledKnown;
}

export default function agentMonitor(pi) {
  const turns = new Set(); // session ids with an open turn
  let unpatch;

  const open = (ctx) => turns.has(sessionId(ctx));
  const begin = (ctx) => { turns.add(sessionId(ctx)); send(ctx, "UserPromptSubmit"); };
  const stop = (ctx) => { if (turns.delete(sessionId(ctx))) send(ctx, "Stop"); };

  // Pi has no permission event: a dialog during a turn is the agent waiting on the user.
  const patchDialogs = (ctx) => {
    let ui;
    try { ui = ctx.hasUI ? ctx.ui : undefined; } catch {}
    const key = Symbol.for("agent-monitor.pi.dialogs");
    if (!ui || ui[key]) return undefined;
    const originals = {};
    for (const name of ["confirm", "select", "input"]) {
      const original = ui[name];
      if (typeof original !== "function") continue;
      originals[name] = original;
      ui[name] = function (...args) {
        const asking = open(ctx);
        if (asking) send(ctx, "PermissionRequest", { message: String(args[0] ?? "") });
        const answered = () => { if (asking && open(ctx)) send(ctx, "PostToolUse"); };
        let result;
        try { result = original.apply(this, args); } catch (e) { answered(); throw e; }
        return Promise.resolve(result).finally(answered);
      };
    }
    ui[key] = true;
    return () => { Object.assign(ui, originals); delete ui[key]; };
  };

  pi.on("session_start", (_event, ctx) => {
    turns.delete(sessionId(ctx));
    try { unpatch?.(); } catch {}
    try { unpatch = patchDialogs(ctx); } catch {}
    send(ctx, "SessionStart");
  });
  pi.on("before_agent_start", (_event, ctx) => { begin(ctx); });
  pi.on("agent_start", (_event, ctx) => { if (!open(ctx)) begin(ctx); });
  pi.on("tool_execution_start", (event, ctx) => {
    if (!event || event.parentToolCallId) return;
    if (isSubagentTool(event.toolName)) {
      send(ctx, "SubagentStart", { agent_id: String(event.toolCallId), agent_type: event.toolName, tool_input: toolInput(event.args) });
    } else {
      send(ctx, "PreToolUse", { tool_name: event.toolName, tool_input: toolInput(event.args) }, true);
    }
  });
  pi.on("tool_execution_end", (event, ctx) => {
    if (!event || event.parentToolCallId) return;
    if (isSubagentTool(event.toolName)) {
      send(ctx, "SubagentStop", { agent_id: String(event.toolCallId), agent_type: event.toolName });
    } else {
      send(ctx, "PostToolUse", { tool_name: event.toolName }, true);
    }
  });
  pi.on("agent_end", (_event, ctx) => { if (!emitsSettled()) stop(ctx); });
  pi.on("agent_settled", (_event, ctx) => {
    let idle = true;
    try { idle = typeof ctx.isIdle !== "function" || ctx.isIdle(); } catch {}
    if (idle) stop(ctx);
  });
  pi.on("session_shutdown", async (_event, ctx) => {
    turns.delete(sessionId(ctx));
    try { unpatch?.(); } catch {}
    unpatch = undefined;
    await send(ctx, "SessionEnd");
  });
}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::Harness;
    use crate::status::{Status::*, map_event_to_status};
    use serde_json::json;

    fn ctx(program: &str) -> HookContext {
        HookContext::new(&PI, Some(Path::new(program)))
    }

    #[test]
    fn the_extension_carries_the_marker_and_the_exact_argv() {
        let text = render(&ctx("/opt/am dir/am-hook"));
        assert!(text.starts_with(&format!("// {OWNED_FILE_MARKER}")), "{text}");
        let mut argv = ctx("/opt/am dir/am-hook").argv("", "");
        argv.pop();
        assert_eq!(argv, ["/opt/am dir/am-hook", "hook-event", "--agent", "pi", "--worktree", "", "--event"]);
        assert!(text.contains(&format!("const ARGV = {};\n", serde_json::to_string(&argv).unwrap().replace(",", ", "))), "{text}");
        assert!(text.contains("const TIMEOUT_MS = 5000;\n"));
        assert!(!text.contains("__"), "every placeholder is filled");
        // What `dangling_hook_binaries` / `am doctor` check.
        assert_eq!(crate::hooks_installer::hook_programs_in_text(&text), [PathBuf::from("/opt/am dir/am-hook")]);
        // A runtime import of Pi's package would make the file depend on resolving it.
        assert!(!text.contains("@earendil-works") && !text.contains("@mariozechner"));
        // The CLI fallback runs `am hook-event`.
        let fallback = render(&HookContext::new(&PI, None));
        assert!(fallback.contains(r#"const ARGV = ["am", "hook-event", "--agent", "pi", "--worktree", "", "--event"];"#), "{fallback}");
    }

    #[test]
    fn the_extension_lives_in_the_agent_dir() {
        let home = Path::new("/home/me");
        assert_eq!(agent_dir_from(None, home), Path::new("/home/me/.pi/agent"));
        assert_eq!(agent_dir_from(Some("".into()), home), Path::new("/home/me/.pi/agent"));
        assert_eq!(agent_dir_from(Some("/srv/pi".into()), home), Path::new("/srv/pi"));
        assert_eq!(agent_dir_from(Some("~/alt/pi".into()), home), Path::new("/home/me/alt/pi"));
        assert!(matches!(PI.hook_install(), HookInstall::OwnedFile(OwnedFile { register: None, .. })));
    }

    #[test]
    fn the_payloads_drive_the_status_rules() {
        let step = |cur, p: Value| map_event_to_status(&PI.normalize(&p, None), cur, 0);
        let base = |e: &str| json!({"hook_event_name": e, "session_id": "019e", "cwd": "/w"});
        assert_eq!(step(None, base("SessionStart")), Some(Idle));
        let s = step(Some(Idle), base("UserPromptSubmit"));
        assert_eq!(s, Some(Executing));
        let mut tool = base("PreToolUse");
        tool["tool_name"] = "bash".into();
        tool["tool_input"] = json!({"command": "git push"});
        let e = PI.normalize(&tool, None);
        assert_eq!((e.tool_name.as_deref(), e.session_id.as_deref(), e.cwd.as_deref()), (Some("Bash"), Some("019e"), Some("/w")));
        assert_eq!(step(s, tool), Some(Executing));
        let w = step(s, base("PermissionRequest"));
        assert_eq!(w, Some(Waiting));
        assert_eq!(step(w, base("PostToolUse")), Some(Executing));
        let mut sub = base("SubagentStart");
        sub["agent_id"] = "call_1".into();
        sub["agent_type"] = "subagent".into();
        let e = PI.normalize(&sub, None);
        assert!(e.is_subagent_lifecycle() && !e.from_subagent() && !e.is_internal_agent());
        let d = step(s, base("Stop"));
        assert_eq!(d, Some(Done));
        assert_eq!(step(d, base("Stop")), Some(Idle), "why the extension sends one Stop per turn");
        assert_eq!(step(d, base("SessionEnd")), Some(Idle));
    }

    #[test]
    fn resume_and_process_matching() {
        assert_eq!(PI.resume_args(Some("019e306c"), false), ["--session", "019e306c"]);
        assert_eq!(PI.resume_args(None, true), ["--continue"]);
        let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(PI.matches_process("pi", &argv(&["pi"])));
        // npm's bin symlink runs as `node <prefix>/bin/pi`.
        assert!(PI.matches_process("node", &argv(&["node", "/usr/local/bin/pi", "--continue"])));
        assert!(PI.matches_process(
            "node",
            &argv(&["node", "/u/.nvm/lib/node_modules/@earendil-works/pi-coding-agent/dist/cli.js"])
        ));
        assert!(PI.matches_process("node", &argv(&["node", "/u/lib/node_modules/@mariozechner/pi-coding-agent/dist/cli.js"])));
        assert!(!PI.matches_process("node", &argv(&["node", "/srv/app/pipeline.js"])));
        assert!(!PI.matches_process("pip", &argv(&["pip", "install"])));
    }

    #[test]
    fn discovers_sessions_under_the_root() {
        let dir = tempfile::tempdir().unwrap();
        let mk = |sub: &str, file: &str, header: Value| {
            let d = dir.path().join(sub);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join(file), format!("{header}\n{{\"type\":\"model_change\"}}\n")).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(15));
        };
        let h = |id: &str, cwd: &str| json!({"type": "session", "version": 3, "id": id, "timestamp": "t", "cwd": cwd});
        mk("--w-repo--", "2026-10-01T00-00-00-000Z_aaa.jsonl", h("aaa", "/w/repo"));
        mk("--w-repo-pkg--", "2026-10-02T00-00-00-000Z_bbb.jsonl", h("bbb", "/w/repo/pkg"));
        mk("--w-repo-other--", "2026-10-03T00-00-00-000Z_ccc.jsonl", h("ccc", "/w/repo-other"));
        mk("--w-repo--", "notes.txt", h("ddd", "/w/repo"));
        mk("--w-repo--", "broken.jsonl", json!({"type": "message"}));
        // A custom sessions dir keeps every cwd's sessions side by side.
        mk("", "2026-10-04T00-00-00-000Z_eee.jsonl", h("eee", "/w/repo"));
        mk("", "2026-10-05T00-00-00-000Z_fff.jsonl", h("fff", "/elsewhere"));
        let s = discover_sessions_in(dir.path(), Path::new("/w/repo"));
        let ids: Vec<&str> = s.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["eee", "bbb", "aaa"]);
        assert_eq!(s[1].cwd, "/w/repo/pkg");
        assert!(s.iter().all(|s| s.agent == HarnessKind::PI));
        assert!(discover_sessions_in(&dir.path().join("missing"), Path::new("/w/repo")).is_empty());
    }

    /// Runs the generated extension in Node against a fake Pi, with a fake
    /// am-hook that records each run, and checks what am would be sent.
    /// Skipped when `node` isn't on PATH.
    #[test]
    fn the_extension_runs_in_node() {
        let Some(node) = which("node") else { return };
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("runs.log");
        let hook = dir.path().join("am-hook");
        std::fs::write(&hook, format!("#!/bin/sh\n{{ printf '%s ' \"$@\"; cat; echo; }} >> '{}'\n", log.display())).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(dir.path().join("ext.mjs"), render(&HookContext::new(&PI, Some(&hook)))).unwrap();
        std::fs::write(dir.path().join("driver.mjs"), NODE_DRIVER).unwrap();
        let out = std::process::Command::new(node).arg(dir.path().join("driver.mjs")).current_dir(dir.path()).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let runs = std::fs::read_to_string(&log).unwrap();
        let lines: Vec<(&str, Value)> = runs
            .lines()
            .map(|l| {
                let (args, payload) = l.split_at(l.find('{').unwrap());
                (args, serde_json::from_str(payload).unwrap())
            })
            .collect();
        let events: Vec<&str> = lines.iter().map(|(_, p)| p["hook_event_name"].as_str().unwrap()).collect();
        assert_eq!(
            events,
            [
                "SessionStart", "UserPromptSubmit", "PreToolUse", "PermissionRequest", "PostToolUse", "PostToolUse",
                "SubagentStart", "SubagentStop", "Stop", "UserPromptSubmit", "Stop", "SessionEnd"
            ],
            "{runs}"
        );
        for (args, p) in &lines {
            let e = p["hook_event_name"].as_str().unwrap();
            assert_eq!(*args, format!("hook-event --agent pi --worktree  --event {e} "));
            assert_eq!((p["session_id"].as_str(), p["cwd"].as_str()), (Some("sess-1"), Some("/w/repo")));
        }
        assert_eq!(lines[2].1["tool_name"], "bash");
        assert_eq!(lines[2].1["tool_input"], json!({"command": "git push"}));
        assert_eq!(lines[3].1["message"], "Allow?");
        assert_eq!(lines[6].1["agent_id"], "call_2");
        assert_eq!(lines[6].1["agent_type"], "subagent");
        assert_eq!(lines[6].1["tool_input"], json!({"description": "scan the repo"}));
    }

    fn which(name: &str) -> Option<PathBuf> {
        std::env::var_os("PATH")?.to_str()?.split(':').map(|d| Path::new(d).join(name)).find(|p| p.is_file())
    }

    /// A fake Pi: fires one session's events through the extension. `node`
    /// isn't a Pi package, so the extension settles on `agent_end`, and the
    /// late `agent_settled` must not send a second Stop.
    const NODE_DRIVER: &str = r#"
import ext from "./ext.mjs";
const handlers = {};
const pi = { on: (name, fn) => { (handlers[name] ||= []).push(fn); } };
let answer;
const ui = { confirm: () => new Promise((r) => { answer = r; }), select: async () => undefined, input: async () => undefined };
const confirm = ui.confirm;
const ctx = { cwd: "/w/repo", hasUI: true, ui, sessionManager: { getSessionId: () => "sess-1" }, isIdle: () => true };
const fire = async (name, event = {}) => { for (const fn of handlers[name] || []) await fn({ type: name, ...event }, ctx); };
ext(pi);
await fire("session_start", { reason: "startup" });
await fire("before_agent_start", { prompt: "hi" });
await fire("agent_start");
await fire("tool_execution_start", { toolCallId: "call_1", toolName: "bash", args: { command: "git push", timeout: 5 } });
const asked = ctx.ui.confirm("Allow?", "rm -rf");
await new Promise((r) => setTimeout(r, 50));
answer(true);
await asked;
await fire("tool_execution_start", { toolCallId: "call_1/1", toolName: "read", args: {}, parentToolCallId: "call_1" });
await fire("tool_execution_end", { toolCallId: "call_1", toolName: "bash", result: {}, isError: false });
await fire("tool_execution_start", { toolCallId: "call_2", toolName: "subagent", args: { task: "scan the repo" } });
await fire("tool_execution_end", { toolCallId: "call_2", toolName: "subagent", result: {}, isError: false });
await fire("agent_end", { messages: [] });
await fire("agent_settled", { aborted: false });
await fire("agent_start");
await fire("agent_end", { messages: [] });
await fire("session_shutdown", { reason: "quit" });
if (ui.confirm !== confirm || ui[Symbol.for("agent-monitor.pi.dialogs")]) throw new Error("dialogs still patched");
"#;
}
