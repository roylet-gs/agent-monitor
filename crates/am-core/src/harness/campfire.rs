//! Campfire (a white-label of the Pi coding agent with collaborative
//! sessions) as a built-in plugin: the manifest `campfire.json` plus a
//! generated TypeScript extension in place of hooks.
//!
//! Campfire has no shell hooks; it loads TypeScript extensions from
//! `${CAMPFIRE_CODING_AGENT_DIR:-~/.campfire/agent}/extensions/` and hands
//! them Pi's extension API. `am hooks install --agent campfire` writes
//! `extensions/agent-monitor-campfire.ts` there (an [`OwnedFile`] carrying
//! [`OWNED_FILE_MARKER`], rendered from `campfire-extension.ts`; nothing to
//! register). Campfire loads extensions at startup, so a running session
//! needs a restart to pick it up.
//!
//! The extension maps Campfire's events onto canonical ones (the manifest's
//! events map is the identity) and runs am-hook for each, one at a time,
//! detached, killed after 5 s, never throwing and never making Campfire
//! wait (except up to 1.5 s at shutdown, so `SessionEnd` gets out):
//! - `session_start` → `SessionStart`; `session_end` / Pi's
//!   `session_shutdown` → `SessionEnd` (once).
//! - `before_agent_start` → `UserPromptSubmit`, `agent_end` → `Stop` with the
//!   last assistant text, once per turn, since a second `Stop` would turn
//!   `done` into `idle`.
//! - The collaborative observer bridge (`Symbol.for("campfire.observer.v1")`
//!   on `globalThis`): `join.requested` (someone waits in the lobby) and
//!   `permission.asked` (a collaborator asks for a capability) need the host,
//!   so they are a `Notification` of type `permission_prompt` (waiting; the
//!   rules leave a `done` session alone). There is no event for the answer:
//!   the next prompt moves the row on, and a turn that saw an ask sends
//!   `PostToolUse` before its `Stop` (as `Stop` keeps `waiting`).
//!   `relay.error` is ignored.
//! - Only the host is reported (`CAMPFIRE_SESSION_ROLE=host`): a joiner is an
//!   ephemeral view of someone else's session whose argv carries the invite
//!   token, so the extension sends nothing there and never reads argv.
//!
//! No tool events: Campfire's collaborative layer is what matters here, and
//! a turn is `executing` from prompt to `agent_end`. Resume:
//! `campfire --session <id>` with the ids am's hooks recorded (the session
//! manager's id). Detected when the agent dir exists (`~/.campfire` by
//! default).

use std::path::PathBuf;

use super::embedded::Embedded;
use super::manifest::{Manifest, expand_home};
use crate::hooks_installer::{HookContext, HookInstall, OWNED_FILE_MARKER, OwnedFile, js_string};

pub static CAMPFIRE: Embedded = Embedded::new(|| Manifest::builtin(include_str!("campfire.json")), None)
    .with_hook_file(|| (extension_path(), ""))
    .with_presence(|| agent_dir(&env).is_dir())
    .with_hook_install(|| HookInstall::OwnedFile(OwnedFile { render, register: None }));

const TEMPLATE: &str = include_str!("campfire-extension.ts");
const ARGV_SLOT: &str = "__AM_HOOK_ARGV__";
const COMMAND_SLOT: &str = "__AM_HOOK_COMMAND__";
const FILE_NAME: &str = "agent-monitor-campfire.ts";

/// The extension file am owns.
pub fn extension_path() -> PathBuf {
    agent_dir(&env).join("extensions").join(FILE_NAME)
}

fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().map(|v| v.trim().to_owned()).filter(|v| !v.is_empty())
}

/// Campfire's agent dir: `$CAMPFIRE_CODING_AGENT_DIR`, else `~/.campfire/agent`.
fn agent_dir(get: &dyn Fn(&str) -> Option<String>) -> PathBuf {
    match get("CAMPFIRE_CODING_AGENT_DIR") {
        Some(d) => expand_home(&d),
        None => expand_home("~/.campfire/agent"),
    }
}

/// The extension's source: the template with am-hook's argv up to `--event`
/// filled in (the extension appends the event name).
fn render(ctx: &HookContext) -> String {
    let argv = ctx.argv("", "");
    let head = &argv[..argv.len() - 1];
    let list = head.iter().map(|a| js_string(a)).collect::<Vec<_>>().join(", ");
    // The shell form names the program next to `hook-event`, which is how
    // the dangling-binary scan finds it whatever the binary is called.
    let command = crate::harness::hook_command_line(&ctx.agent_id, None, "<event>", &ctx.invocation);
    debug_assert!(TEMPLATE.contains(OWNED_FILE_MARKER));
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
        HookContext::new(&CAMPFIRE, native)
    }

    #[test]
    fn the_extension_spawns_am_hook_with_the_exact_argv() {
        let text = render(&ctx(Some(Path::new("/opt/am bin/am-hook"))));
        assert!(text.starts_with(&format!("// {OWNED_FILE_MARKER}\n")));
        assert!(!text.contains(ARGV_SLOT) && !text.contains(COMMAND_SLOT));
        assert!(text.contains("// \"/opt/am bin/am-hook\" hook-event --agent campfire --worktree \"\" --event <event>\n"), "{text}");
        let head = r#"const HOOK = ["/opt/am bin/am-hook", "hook-event", "--agent", "campfire", "--worktree", "", "--event"];"#;
        assert!(text.contains(head), "{text}");
        assert_eq!(text.matches("export ").count(), 1);
        assert!(!text.contains("process.argv"), "never reads argv (a joiner's holds the invite token)");
        // The CLI fallback runs `am hook-event`, which also reads stdin.
        assert!(render(&ctx(None)).contains(r#"const HOOK = ["am", "hook-event", "--agent", "campfire""#));
        let c = ctx(Some(Path::new("/b/am-hook")));
        assert_eq!(c.argv("Stop", ""), ["/b/am-hook", "hook-event", "--agent", "campfire", "--worktree", "", "--event", "Stop"]);
        // The dangling-binary scan finds the program in the file.
        let mut found = crate::hooks_installer::hook_programs_in_text(&render(&c));
        found.dedup();
        assert_eq!(found, [PathBuf::from("/b/am-hook")]);
        let odd = render(&ctx(Some(Path::new("/x/my-hook"))));
        assert!(crate::hooks_installer::hook_programs_in_text(&odd).contains(&PathBuf::from("/x/my-hook")));
    }

    #[test]
    fn the_agent_dir_follows_campfires_override() {
        let home = crate::paths::home_dir();
        assert_eq!(agent_dir(&|_| None), home.join(".campfire/agent"));
        let own = |k: &str| (k == "CAMPFIRE_CODING_AGENT_DIR").then(|| "/cf/agent".to_owned());
        assert_eq!(agent_dir(&own), PathBuf::from("/cf/agent"));
        let tilde = |_: &str| Some("~/elsewhere".to_owned());
        assert_eq!(agent_dir(&tilde), home.join("elsewhere"));
    }

    #[test]
    fn the_extensions_payloads_drive_the_status_rules() {
        let step = |cur, p: Value| map_event_to_status(&CAMPFIRE.normalize(&p, None), cur, 0);
        let p = |event: &str| json!({"session_id": "cf1", "cwd": "/w", "hook_event_name": event});
        let e = CAMPFIRE.normalize(&p("UserPromptSubmit"), None);
        assert_eq!((e.event.as_str(), e.session_id.as_deref(), e.cwd.as_deref()), ("UserPromptSubmit", Some("cf1"), Some("/w")));
        assert_eq!(step(None, p("SessionStart")), Some(Idle));
        let s = step(Some(Idle), p("UserPromptSubmit"));
        assert_eq!(s, Some(Executing));
        let mut ask = p("Notification");
        ask["notification_type"] = "permission_prompt".into();
        ask["message"] = "Ana is waiting to join the Campfire session".into();
        let w = step(s, ask.clone());
        assert_eq!(w, Some(Waiting));
        // The turn ends: PostToolUse first, so the Stop lands on done.
        let s = step(w, p("PostToolUse"));
        assert_eq!(s, Some(Executing));
        assert_eq!(step(s, p("Stop")), Some(Done));
        // A second Stop would have turned it idle (why the extension sends one).
        assert_eq!(step(Some(Done), p("Stop")), Some(Idle));
        // An ask while idle waits; a finished session is left alone.
        assert_eq!(step(Some(Idle), ask.clone()), Some(Waiting));
        assert_eq!(step(Some(Done), ask), None);
        assert_eq!(step(Some(Waiting), p("SessionEnd")), Some(Idle));
    }

    #[test]
    fn resume_processes_and_install_style() {
        assert_eq!(CAMPFIRE.resume_args(Some("cf1"), false), ["--session", "cf1"]);
        assert!(CAMPFIRE.resume_args(None, true).is_empty());
        assert_eq!(CAMPFIRE.program(), "campfire");
        assert_eq!(HarnessKind::get("campfire"), Some(HarnessKind::CAMPFIRE));
        let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(CAMPFIRE.matches_process("campfire", &argv(&["campfire"])));
        assert!(CAMPFIRE.matches_process("bun", &argv(&["bun", "/opt/campfire/campfire.js", "--session", "x"])));
        assert!(!CAMPFIRE.matches_process("node", &argv(&["node", "server.js"])));
        assert!(!CAMPFIRE.matches_process("pi", &argv(&["pi"])));
        assert!(matches!(CAMPFIRE.hook_install(), HookInstall::OwnedFile(OwnedFile { register: None, .. })));
    }

    fn node() -> bool {
        std::process::Command::new("node").arg("--version").output().is_ok_and(|o| o.status.success())
    }

    #[test]
    fn the_extension_parses_as_an_es_module() {
        if !node() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("agent-monitor-campfire.mjs");
        std::fs::write(&file, render(&ctx(Some(Path::new("/b/am-hook"))))).unwrap();
        let check = std::process::Command::new("node").arg("--check").arg(&file).output().unwrap();
        assert!(check.status.success(), "{}", String::from_utf8_lossy(&check.stderr));
    }

    /// Runs the rendered extension under node (it is JS-compatible TS)
    /// against a fake am-hook that logs its argv and stdin, feeding it a fake
    /// Pi extension API. Skipped without node.
    #[test]
    fn the_extension_sends_one_hook_per_state_change_in_order() {
        use std::os::unix::fs::PermissionsExt;
        if !node() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("hooks.log");
        let hook = dir.path().join("am-hook");
        std::fs::write(&hook, format!("#!/bin/sh\n{{ printf '%s ' \"$@\"; printf '\\t'; cat; printf '\\n'; }} >> '{}'\n", log.display())).unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        let ext = dir.path().join("agent-monitor-campfire.mjs");
        std::fs::write(&ext, render(&ctx(Some(&hook)))).unwrap();
        let driver = dir.path().join("driver.mjs");
        std::fs::write(
            &driver,
            r#"import * as fs from "node:fs";
const [ext, log, role] = process.argv.slice(2);
if (role) process.env.CAMPFIRE_SESSION_ROLE = role; else delete process.env.CAMPFIRE_SESSION_ROLE;
const { default: extension } = await import(ext);
const handlers = new Map();
const api = { on: (name, h) => handlers.set(name, h) };
extension(api);
// A second load (a reload) must replace the observer listener, not add one.
extension(api);
const ctx = { cwd: "/w/repo", sessionManager: { getSessionId: () => "cf-1" } };
const fire = async (name, event = {}) => { const h = handlers.get(name); if (h) await h(event, ctx); };
const observe = (e) => { const b = globalThis[Symbol.for("campfire.observer.v1")]; if (b) for (const l of b.listeners) l(e); };
await fire("session_start", { type: "session_start" });
await fire("before_agent_start", { prompt: "secret prompt" });
await fire("before_agent_start", { prompt: "again" });
observe({ type: "join.requested", displayName: "Ana" });
observe({ type: "relay.error", reason: "down" });
observe({ type: "presence.changed" });
await fire("agent_end", { messages: [{ role: "user", content: "hi" }, { role: "assistant", content: [{ type: "text", text: "All done." }] }] });
await fire("agent_end", { messages: [] });
observe({ type: "permission.asked", displayName: "Bo", capability: "shell:exec" });
await fire("before_agent_start", {});
await fire("agent_end", {});
await fire("session_shutdown", {});
await fire("session_end", {});
const want = role === "host" ? 9 : 0;
const start = Date.now();
const lines = () => { try { return fs.readFileSync(log, "utf8").split("\n").filter(Boolean).length; } catch { return 0; } };
while (lines() < want && Date.now() - start < 8000) await new Promise((r) => setTimeout(r, 25));
await new Promise((r) => setTimeout(r, 200));
"#,
        )
        .unwrap();
        let run = |role: &str| {
            let out = std::process::Command::new("node").arg(&driver).arg(&ext).arg(&log).arg(role).output().unwrap();
            assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        };
        // A joiner (or no role at all) reports nothing.
        run("joiner");
        run("");
        assert!(!log.exists(), "{:?}", std::fs::read_to_string(&log));
        run("host");
        let text = std::fs::read_to_string(&log).unwrap();
        let sent: Vec<(String, Value)> = text
            .lines()
            .map(|l| {
                let (args, payload) = l.split_once('\t').unwrap();
                let args = args.trim_end();
                assert!(args.starts_with("hook-event --agent campfire --worktree  --event "), "{args}");
                (args.rsplit(' ').next().unwrap().to_owned(), serde_json::from_str(payload).unwrap())
            })
            .collect();
        let events: Vec<&str> = sent.iter().map(|(e, _)| e.as_str()).collect();
        assert_eq!(
            events,
            ["SessionStart", "UserPromptSubmit", "Notification", "PostToolUse", "Stop", "Notification", "UserPromptSubmit", "Stop", "SessionEnd"]
        );
        for (event, p) in &sent {
            assert_eq!(p["hook_event_name"], event.as_str());
            assert_eq!((&p["session_id"], &p["cwd"]), (&json!("cf-1"), &json!("/w/repo")));
            assert!(p.get("prompt").is_none(), "prompts are never sent");
        }
        assert_eq!(sent[2].1["notification_type"], "permission_prompt");
        assert_eq!(sent[2].1["message"], "Ana is waiting to join the Campfire session");
        assert_eq!(sent[4].1["last_assistant_message"], "All done.");
        assert_eq!(sent[5].1["message"], "Bo asked for permission: shell:exec");
        assert!(sent[7].1.get("last_assistant_message").is_none());
    }
}
