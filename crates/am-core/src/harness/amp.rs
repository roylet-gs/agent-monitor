//! Amp (Sourcegraph's coding agent, `amp`) as a built-in plugin: the manifest
//! `amp.json` plus a generated TypeScript plugin in place of hooks.
//!
//! Amp has no shell hooks; its CLI loads TS/JS plugins (run with Bun) from
//! `.amp/plugins/` in a project and `$XDG_CONFIG_HOME/amp/plugins/` (else
//! `~/.config/amp/plugins/`) system-wide, and hands each one a `PluginAPI`.
//! `am hooks install --agent amp` writes `<that dir>/agent-monitor.ts` (an
//! [`OwnedFile`] carrying [`OWNED_FILE_MARKER`], rendered from
//! `amp-plugin.ts`). Quirks:
//! - The plugin API is experimental: Amp only loads plugins when started
//!   with `PLUGINS=all`, only in the CLI (not the editor extension), and the
//!   file's first line must be
//!   `// @i-know-the-amp-plugin-api-is-wip-and-very-experimental-right-now`.
//! - The template is plain JavaScript with no `import type` from
//!   `@ampcode/plugin`, so nothing has to resolve at runtime; the default
//!   export is the plugin function and `description` a static string.
//! - The plugin process may not run in the thread's directory, so `cwd` is
//!   `amp.system.workspaceRoot` (a URI), else `$PWD`, else `process.cwd()`.
//! - The thread id (`T-…`) is the `session_id`. There is no session-end
//!   event; a thread stays open until Amp's process goes (the process scan
//!   drops sessions beyond the running `amp` count).
//!
//! The plugin runs am-hook once per state change, one at a time, detached,
//! with a 5 s kill, stdin JSON closed, a minimal environment (no
//! `AMP_API_KEY`) and never throwing into Amp:
//! - `session.start` → `SessionStart`; `agent.start` → `UserPromptSubmit`
//!   (once per turn).
//! - `tool.call` / `tool.result` → `PreToolUse` / `PostToolUse` with Amp's
//!   tool name (mapped onto Claude's in `amp.json`: `edit_file` → `Edit`…)
//!   and only the arguments am summarises. `tool.call` always answers
//!   `allow`, which never overrides another plugin. Tool events of threads
//!   that never started in this process (a subagent's) are dropped.
//! - The thread's `state` observable: `awaiting-approval` (or
//!   `awaiting-input`) → `PermissionRequest` (waiting); `running` again →
//!   `PostToolUse` (executing).
//! - `agent.end` records the outcome; the turn ends when `state` is `idle`
//!   (or at once when the thread has no `state` observable): `Stop`
//!   (done, cancelled) or `StopFailure` (error), once per turn, since a
//!   second `Stop` would turn `done` into `idle`. Ending while waiting sends
//!   `PostToolUse` first, since `Stop` keeps `waiting`.
//!
//! Threads live on Amp's servers, so resume (`amp threads continue <id>`)
//! offers the sessions am's hooks recorded.

use std::path::PathBuf;

use super::embedded::Embedded;
use super::manifest::{Manifest, expand_home};
use crate::hooks_installer::{HookContext, HookInstall, OWNED_FILE_MARKER, OwnedFile, js_string};

pub static AMP: Embedded = Embedded::new(|| Manifest::builtin(include_str!("amp.json")), None)
    .with_hook_file(|| (plugin_path(), ""))
    .with_hook_install(|| HookInstall::OwnedFile(OwnedFile { render, register: None }));

const TEMPLATE: &str = include_str!("amp-plugin.ts");
const ARGV_SLOT: &str = "__AM_HOOK_ARGV__";
const COMMAND_SLOT: &str = "__AM_HOOK_COMMAND__";
/// Amp refuses a plugin file without this line.
pub const WIP_LINE: &str = "// @i-know-the-amp-plugin-api-is-wip-and-very-experimental-right-now";

/// The plugin file am owns.
pub fn plugin_path() -> PathBuf {
    plugins_dir(&|k| std::env::var(k).ok().map(|v| v.trim().to_owned()).filter(|v| !v.is_empty())).join("agent-monitor.ts")
}

/// Amp's system plugin dir: `$XDG_CONFIG_HOME/amp/plugins`, else
/// `~/.config/amp/plugins`.
fn plugins_dir(get: &dyn Fn(&str) -> Option<String>) -> PathBuf {
    match get("XDG_CONFIG_HOME") {
        Some(x) => expand_home(&x).join("amp/plugins"),
        None => expand_home("~/.config/amp/plugins"),
    }
}

/// The plugin's source: the template with am-hook's argv up to `--event`
/// filled in (the plugin appends the event name).
fn render(ctx: &HookContext) -> String {
    let argv = ctx.argv("", "");
    let head = &argv[..argv.len() - 1];
    let list = head.iter().map(|a| js_string(a)).collect::<Vec<_>>().join(", ");
    // The shell form names the program next to `hook-event`, which is how
    // the dangling-binary scan finds it whatever the binary is called.
    let command = crate::harness::hook_command_line(&ctx.agent_id, None, "<event>", &ctx.invocation);
    debug_assert!(TEMPLATE.starts_with(WIP_LINE) && TEMPLATE.contains(OWNED_FILE_MARKER));
    TEMPLATE.replace(COMMAND_SLOT, &command).replace(ARGV_SLOT, &list)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{Harness, HarnessKind};
    use crate::status::{Status::*, map_event_to_status};
    use serde_json::{Value, json};
    use std::path::Path;

    fn ctx(native: Option<&Path>) -> HookContext {
        HookContext::new(&AMP, native)
    }

    fn has_node() -> bool {
        std::process::Command::new("node").arg("--version").output().is_ok_and(|o| o.status.success())
    }

    #[test]
    fn the_plugin_spawns_am_hook_with_the_exact_argv() {
        let text = render(&ctx(Some(Path::new("/opt/am bin/am-hook"))));
        assert!(text.starts_with(&format!("{WIP_LINE}\n")), "Amp wants the WIP line first");
        assert!(text.contains(OWNED_FILE_MARKER));
        assert!(!text.contains(ARGV_SLOT) && !text.contains(COMMAND_SLOT));
        assert!(text.contains("// \"/opt/am bin/am-hook\" hook-event --agent amp --worktree \"\" --event <event>\n"), "{text}");
        let head = r#"const HOOK = ["/opt/am bin/am-hook", "hook-event", "--agent", "amp", "--worktree", "", "--event"];"#;
        assert!(text.contains(head), "{text}");
        assert!(text.contains("export default function"));
        assert!(!text.contains("@ampcode/plugin\""), "no runtime import of the plugin package");
        assert!(!text.contains("AMP_API_KEY"), "the hook env is an allow-list");
        // The CLI fallback runs `am hook-event`, which also reads stdin.
        assert!(render(&ctx(None)).contains(r#"const HOOK = ["am", "hook-event", "--agent", "amp""#));
        let c = ctx(Some(Path::new("/b/am-hook")));
        assert_eq!(c.argv("Stop", ""), ["/b/am-hook", "hook-event", "--agent", "amp", "--worktree", "", "--event", "Stop"]);
        // The dangling-binary scan finds the program in the file…
        let mut found = crate::hooks_installer::hook_programs_in_text(&render(&c));
        found.dedup();
        assert_eq!(found, [PathBuf::from("/b/am-hook")]);
        // …also when the binary has another name ($AM_HOOK_BIN).
        let odd = render(&ctx(Some(Path::new("/x/my-hook"))));
        assert!(crate::hooks_installer::hook_programs_in_text(&odd).contains(&PathBuf::from("/x/my-hook")));
    }

    #[test]
    fn the_plugin_dir_follows_xdg_config_home() {
        assert_eq!(plugins_dir(&|_| None), crate::paths::home_dir().join(".config/amp/plugins"));
        let xdg = |k: &str| (k == "XDG_CONFIG_HOME").then(|| "/x/cfg".to_owned());
        assert_eq!(plugins_dir(&xdg), PathBuf::from("/x/cfg/amp/plugins"));
        assert!(plugin_path().ends_with("amp/plugins/agent-monitor.ts"));
        assert_eq!(AMP.settings_path(), plugin_path());
    }

    #[test]
    fn the_plugin_parses_as_an_es_module() {
        if !has_node() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("agent-monitor.mjs");
        std::fs::write(&file, render(&ctx(Some(Path::new("/b/am-hook"))))).unwrap();
        let check = std::process::Command::new("node").arg("--check").arg(&file).output().unwrap();
        assert!(check.status.success(), "{}", String::from_utf8_lossy(&check.stderr));
    }

    /// Runs the rendered plugin under node with a fake `PluginAPI` and a fake
    /// am-hook that logs its argv, stdin and environment. Skipped without node.
    #[test]
    fn the_plugin_sends_one_hook_per_state_change_in_order() {
        use std::os::unix::fs::PermissionsExt;
        if !has_node() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("hooks.log");
        let hook = dir.path().join("am-hook");
        std::fs::write(
            &hook,
            format!("#!/bin/sh\n{{ printf '%s ' \"$@\"; printf '\\t'; cat; printf '\\t%s' \"${{AMP_API_KEY:-none}}\"; printf '\\n'; }} >> '{}'\n", log.display()),
        )
        .unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        let plugin = dir.path().join("agent-monitor.mjs");
        std::fs::write(&plugin, render(&ctx(Some(&hook)))).unwrap();
        let driver = dir.path().join("driver.mjs");
        std::fs::write(
            &driver,
            r#"import * as fs from "node:fs";
const [plugin, log] = process.argv.slice(2);
const mod = await import(plugin);
if (typeof mod.description !== "string") throw new Error("description");
// A fake PluginAPI: handlers by event, one state observable per thread.
const handlers = {};
const states = {};
const observable = () => {
  const subs = new Set();
  let value = "idle";
  return {
    get: async () => value,
    subscribe: (cb) => { subs.add(cb); return { unsubscribe: () => subs.delete(cb) }; },
    set: (v) => { value = v; for (const cb of subs) cb(v); },
  };
};
const thread = (id) => ({ id, state: (states[id] ||= observable()) });
const amp = {
  on: (name, fn) => { handlers[name] = fn; },
  system: { workspaceRoot: "file:///w/my%20repo" },
  helpers: {
    filePathFromURI: (u) => decodeURIComponent(new URL(String(u)).pathname),
    shellCommandFromToolCall: (e) => (e.tool === "Bash" ? { command: e.input.cmd } : null),
  },
  threads: { get: (id) => thread(id) },
};
mod.default(amp);
const T = "T-1";
const on = (name, event, ctx) => handlers[name](event, ctx);
await on("session.start", { thread: { id: T } });
await on("agent.start", { thread: { id: T }, message: "secret prompt", id: 1 });
states[T].set("running");
const allow = await on("tool.call", { toolUseID: "u1", tool: "Bash", input: { cmd: "cargo test", content: "x".repeat(9000) }, thread: { id: T } });
if (allow.action !== "allow") throw new Error("tool.call must allow");
states[T].set("awaiting-approval");
states[T].set("awaiting-approval");
states[T].set("running");
await on("tool.result", { toolUseID: "u1", tool: "Bash", input: { cmd: "cargo test" }, status: "done", thread: { id: T } });
await on("tool.call", { toolUseID: "u2", tool: "edit_file", input: { path: "/w/my repo/a.rs" }, thread: { id: "T-sub" } });
await on("agent.end", { thread: { id: T }, status: "done", messages: [] });
states[T].set("idle");
states[T].set("idle");
await on("tool.result", { toolUseID: "u1", tool: "Bash", input: {}, status: "done", thread: { id: T } });
// A second turn that ends in an error while waiting on approval.
await on("agent.start", { thread: { id: T }, message: "again", id: 2 });
states[T].set("running");
states[T].set("awaiting-approval");
await on("agent.end", { thread: { id: T }, status: "error", messages: [] });
states[T].set("idle");
// A thread without a state observable ends its turn on agent.end.
amp.threads.get = () => ({ id: "T-2" });
await on("session.start", { thread: { id: "T-2" } });
await on("agent.start", { thread: { id: "T-2" } });
await on("agent.end", { thread: { id: "T-2" }, status: "cancelled" });
await on("agent.end", { thread: { id: "T-2" }, status: "done" });
await on("session.start", {});
const want = 14;
const start = Date.now();
const lines = () => { try { return fs.readFileSync(log, "utf8").split("\n").filter(Boolean).length; } catch { return 0; } };
while (lines() < want && Date.now() - start < 8000) await new Promise((r) => setTimeout(r, 25));
await new Promise((r) => setTimeout(r, 200));
"#,
        )
        .unwrap();
        let out = std::process::Command::new("node")
            .arg(&driver)
            .arg(&plugin)
            .arg(&log)
            .env("AMP_API_KEY", "sk-secret")
            .env("PWD", "/elsewhere")
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let text = std::fs::read_to_string(&log).unwrap();
        assert!(!text.contains("sk-secret") && !text.contains("secret prompt"), "{text}");
        let sent: Vec<(String, Value)> = text
            .lines()
            .map(|l| {
                let mut parts = l.split('\t');
                let args = parts.next().unwrap().trim_end();
                assert!(args.starts_with("hook-event --agent amp --worktree  --event "), "{args}");
                (args.rsplit(' ').next().unwrap().to_owned(), serde_json::from_str(parts.next().unwrap()).unwrap())
            })
            .collect();
        let events: Vec<&str> = sent.iter().map(|(e, _)| e.as_str()).collect();
        assert_eq!(
            events,
            [
                "SessionStart",
                "UserPromptSubmit",
                "PreToolUse",
                "PermissionRequest",
                "PostToolUse",
                "PostToolUse",
                "Stop",
                "UserPromptSubmit",
                "PermissionRequest",
                "PostToolUse",
                "StopFailure",
                "SessionStart",
                "UserPromptSubmit",
                "Stop",
            ]
        );
        for (i, (event, p)) in sent.iter().enumerate() {
            assert_eq!(p["hook_event_name"], event.as_str());
            assert_eq!(p["session_id"], if i < 11 { "T-1" } else { "T-2" }, "{p}");
            assert_eq!(p["cwd"], "/w/my repo", "the workspace root wins over $PWD");
        }
        assert_eq!(sent[2].1["tool_name"], "Bash");
        assert_eq!(sent[2].1["tool_input"], json!({"command": "cargo test"}), "only what am summarises");
        assert_eq!(sent[3].1["tool_name"], "Bash", "the approval is for the last tool");
        assert!(sent[4].1.get("tool_name").is_none());
        assert_eq!(sent[5].1["tool_name"], "Bash");
    }

    #[test]
    fn the_plugins_payloads_drive_the_status_rules() {
        let step = |cur, p: Value| map_event_to_status(&AMP.normalize(&p, None), cur, 0);
        let p = |event: &str| json!({"session_id": "T-1", "cwd": "/w", "hook_event_name": event});
        let e = AMP.normalize(&p("UserPromptSubmit"), None);
        assert_eq!((e.event.as_str(), e.session_id.as_deref(), e.cwd.as_deref()), ("UserPromptSubmit", Some("T-1"), Some("/w")));
        assert_eq!(step(None, p("SessionStart")), Some(Idle));
        let s = step(Some(Idle), p("UserPromptSubmit"));
        assert_eq!(s, Some(Executing));
        let mut tool = p("PreToolUse");
        tool["tool_name"] = "edit_file".into();
        tool["tool_input"] = json!({"file_path": "/w/a.rs"});
        let e = AMP.normalize(&tool, None);
        assert_eq!(e.tool_name.as_deref(), Some("Edit"));
        let s = step(s, p("PermissionRequest"));
        assert_eq!(s, Some(Waiting));
        let s = step(s, p("PostToolUse"));
        assert_eq!(s, Some(Executing));
        assert_eq!(step(s, p("Stop")), Some(Done));
        assert_eq!(step(s, p("StopFailure")), Some(Idle));
    }

    #[test]
    fn resume_and_processes() {
        assert_eq!(AMP.resume_args(Some("T-abc"), false), ["threads", "continue", "T-abc"]);
        assert_eq!(AMP.resume_args(Some("T-abc"), true), ["threads", "continue", "T-abc"]);
        assert!(AMP.resume_args(None, true).is_empty());
        assert_eq!(AMP.program(), "amp");
        assert_eq!(HarnessKind::get("amp"), Some(HarnessKind::AMP));
        let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(AMP.matches_process("amp", &argv(&["amp"])));
        assert!(AMP.matches_process("node", &argv(&["node", "/usr/lib/node_modules/@sourcegraph/amp/dist/main.js"])));
        assert!(!AMP.matches_process("node", &argv(&["node", "server.js"])));
        assert!(!AMP.matches_process("ampd", &argv(&["ampd"])));
        assert!(matches!(AMP.hook_install(), HookInstall::OwnedFile(OwnedFile { register: None, .. })));
    }
}
