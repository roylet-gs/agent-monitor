//! OMP (oh-my-pi, `@oh-my-pi/pi-coding-agent`, binary `omp`, a fork of the
//! Pi coding agent) as a built-in plugin: the manifest `omp.json` plus an
//! owned-file hook install.
//!
//! OMP has no shell hooks; it loads TypeScript extensions from
//! `<agent dir>/extensions/*.ts`. `am hooks install --agent omp` writes
//! `agent-monitor-omp.ts` there ([`render_extension`], from `omp.ts`): it
//! subscribes to OMP's events and runs am-hook (`--agent omp --worktree ""
//! --event <native>`, detached, the JSON payload on stdin, killed after the
//! hook timeout) with a Claude-shaped payload (`session_id`, `cwd`,
//! `hook_event_name`, `tool_name`, …), so the manifest needs no `fields`.
//! The file name is distinct from the Pi plugin's because both CLIs read
//! `PI_CODING_AGENT_DIR`, and the file reports nothing outside omp.
//!
//! Quirks:
//! - **Agent dir:** `$PI_CODING_AGENT_DIR`, else
//!   `~/${PI_CONFIG_DIR:-.omp}/agent`, as OMP's `dirs.ts` resolves it for the
//!   default profile (`OMP_AGENT_DIR` is only cmux's bridge and OMP's own
//!   tests; omp never reads it). A named profile (`OMP_PROFILE`) uses
//!   `~/.omp/profiles/<name>/agent`, which am can't see from outside omp: set
//!   `PI_CODING_AGENT_DIR` to it when installing.
//! - **Detection** is from OMP's own evidence only: `~/${PI_CONFIG_DIR:-.omp}`
//!   or `omp` on `$PATH`, never the agent dir, since Pi reads
//!   `PI_CODING_AGENT_DIR` too and a Pi-only user would otherwise get OMP.
//! - **Ordering:** hooks run one at a time in the order OMP fired them (a FIFO
//!   chain on `globalThis`), so a `PreToolUse` can't land after its
//!   `PermissionRequest` or a `PostToolUse` after the `Stop`.
//! - **One module per session:** OMP loads a fresh copy of the extension for
//!   the main session and each in-process task subagent, so ownership lives
//!   on `globalThis`: the first top-level session owns the terminal; other
//!   sessions report as its subagents (`subagent_start`/`subagent_stop` with
//!   `agent_id` = their session id, `agent_type` `task`).
//! - **Session switches** (`/new`, resume, fork, `session_branch`) end the old
//!   session and start the new one; OMP re-emits `session_switch` for the same
//!   session on reload, which is ignored.
//! - **Stop:** `agent_end` with `willContinue: true` (retry, queued message,
//!   background job) is skipped, and only one Stop goes out per turn (a
//!   second would turn `done` into `idle`).
//! - **Waiting:** `tool_approval_requested` → `PermissionRequest`,
//!   `tool_approval_resolved` → `PostToolUse` (working again). The plan
//!   review opens as the planning turn is aborted, so `plan_review_requested`
//!   is sent as `PreToolUse` of `plan_review` → `ExitPlanMode`, which shows
//!   `waiting` even after the turn's `done`; a dismissed review stays
//!   `waiting` until the next prompt. The `ask` tool → `AskUserQuestion`
//!   (`waiting`); its `tool_execution_end` goes without the tool name, so the
//!   answer reads as working again instead of `waiting` past the turn.
//! - Resume is `omp --session <id>` / `omp --continue`; the sessions offered
//!   are the ones am's hooks recorded.

use std::path::{Path, PathBuf};

use super::embedded::Embedded;
use super::manifest::Manifest;
use crate::hooks_installer::{HookContext, HookInstall, OwnedFile, js_string};
use crate::paths;

pub static OMP: Embedded = Embedded::new(|| Manifest::builtin(include_str!("omp.json")), None)
    .with_hook_file(hook_file)
    .with_hook_install(hook_install)
    .with_presence(is_present);

/// The extension's file name in `<agent dir>/extensions/`.
pub const EXTENSION_FILE: &str = "agent-monitor-omp.ts";

const TEMPLATE: &str = include_str!("omp.ts");

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().map(|v| v.trim().to_owned()).filter(|v| !v.is_empty())
}

fn hook_file() -> (PathBuf, &'static str) {
    (agent_dir(env, &paths::home_dir()).join("extensions").join(EXTENSION_FILE), "")
}

fn hook_install() -> HookInstall {
    HookInstall::OwnedFile(OwnedFile { render: render_extension, register: None })
}

/// OMP's own evidence only: its config root or `omp` on `$PATH`. Not the
/// agent dir, which `PI_CODING_AGENT_DIR` (Pi's variable too) can move.
fn is_present() -> bool {
    config_root(env, &paths::home_dir()).is_dir() || crate::mcp_installer::on_path("omp")
}

/// `~` / `~/x` → under `home`.
fn tilde(p: &str, home: &Path) -> PathBuf {
    match p.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None if p == "~" => home.to_path_buf(),
        None => PathBuf::from(p),
    }
}

/// `~/${PI_CONFIG_DIR:-.omp}` (OMP joins the value onto the home dir).
fn config_root(env: impl Fn(&str) -> Option<String>, home: &Path) -> PathBuf {
    let dir = env("PI_CONFIG_DIR").unwrap_or_else(|| ".omp".into());
    match dir.strip_prefix('~') {
        Some(_) => tilde(&dir, home),
        None => home.join(dir.trim_start_matches('/')),
    }
}

/// OMP's agent dir for the default profile, as omp resolves it.
fn agent_dir(env: impl Fn(&str) -> Option<String>, home: &Path) -> PathBuf {
    match env("PI_CODING_AGENT_DIR") {
        Some(dir) => tilde(&dir, home),
        None => config_root(env, home).join("agent"),
    }
}

/// The extension's source: `omp.ts` with each hook's argv
/// (`ctx.argv(event, "")`) and the timeout filled in.
pub fn render_extension(ctx: &HookContext) -> String {
    let hooks = ctx
        .hooks
        .iter()
        .map(|h| {
            let argv: Vec<String> = ctx.argv(&h.event, "").iter().map(|a| js_string(a)).collect();
            format!("  {}: [{}],", js_string(&h.event), argv.join(", "))
        })
        .collect::<Vec<_>>()
        .join("\n");
    let timeout = ctx.hooks.first().map(|h| h.timeout_ms).unwrap_or(5_000);
    TEMPLATE.replace("__AM_HOOKS__", &hooks).replace("__AM_TIMEOUT_MS__", &timeout.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{Harness, HarnessKind};
    use crate::hooks_installer::OWNED_FILE_MARKER;
    use crate::status::{Status::*, map_event_to_status};
    use serde_json::{Value, json};

    fn ctx(program: Option<&str>) -> HookContext {
        HookContext::new(&OMP, program.map(Path::new))
    }

    fn vars<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |k| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string()).filter(|v| !v.is_empty())
    }

    #[test]
    fn is_registered_as_an_owned_file_harness() {
        assert_eq!(HarnessKind::get("omp").map(|k| k.id()), Some("omp"));
        assert!(matches!(OMP.hook_install(), HookInstall::OwnedFile(OwnedFile { register: None, .. })));
        assert_eq!(OMP.program(), "omp");
        assert_eq!(OMP.short_name(), "OMP");
        assert!(OMP.settings_path().ends_with("extensions/agent-monitor-omp.ts"));
        assert_eq!(
            OMP.hook_events(),
            [
                "session_start",
                "before_agent_start",
                "tool_execution_start",
                "tool_execution_end",
                "tool_approval_requested",
                "tool_approval_resolved",
                "plan_review_requested",
                "subagent_start",
                "subagent_stop",
                "agent_end",
                "session_shutdown"
            ]
        );
    }

    #[test]
    fn agent_dir_follows_omps_environment() {
        let home = Path::new("/h");
        assert_eq!(agent_dir(vars(&[]), home), Path::new("/h/.omp/agent"));
        assert_eq!(agent_dir(vars(&[("PI_CONFIG_DIR", ".ohmy")]), home), Path::new("/h/.ohmy/agent"));
        assert_eq!(agent_dir(vars(&[("PI_CONFIG_DIR", "/abs")]), home), Path::new("/h/abs/agent"), "OMP joins it onto home");
        assert_eq!(agent_dir(vars(&[("PI_CODING_AGENT_DIR", "~/pi-agent")]), home), Path::new("/h/pi-agent"));
        assert_eq!(
            agent_dir(vars(&[("PI_CODING_AGENT_DIR", "/pi"), ("OMP_AGENT_DIR", "/omp")]), home),
            Path::new("/pi"),
            "omp never reads OMP_AGENT_DIR"
        );
        assert_eq!(agent_dir(vars(&[("OMP_AGENT_DIR", "/omp")]), home), Path::new("/h/.omp/agent"));
        assert_eq!(agent_dir(vars(&[("PI_CODING_AGENT_DIR", ""), ("PI_CONFIG_DIR", "")]), home), Path::new("/h/.omp/agent"), "empty = unset");
        assert_eq!(config_root(vars(&[]), home), Path::new("/h/.omp"));
    }

    #[test]
    fn a_pi_agent_dir_alone_isnt_omp() {
        // A Pi-only user: PI_CODING_AGENT_DIR exists, no ~/.omp. Detection
        // looks at the config root, which that variable doesn't move.
        let home = tempfile::tempdir().unwrap();
        let pi = home.path().join("pi-agent");
        std::fs::create_dir_all(&pi).unwrap();
        let pi = pi.display().to_string();
        let pairs = [("PI_CODING_AGENT_DIR", pi.as_str())];
        let env = vars(&pairs);
        assert!(agent_dir(&env, home.path()).is_dir());
        assert!(!config_root(&env, home.path()).is_dir());
        std::fs::create_dir_all(home.path().join(".omp")).unwrap();
        assert!(config_root(&env, home.path()).is_dir());
    }

    #[test]
    fn the_extension_spawns_am_hook_with_exact_argv() {
        let text = render_extension(&ctx(Some("/opt/am bin/am-hook")));
        assert!(text.starts_with(&format!("// {OWNED_FILE_MARKER}\n")), "{text}");
        assert!(!text.contains("__AM_"), "every placeholder is filled");
        assert!(
            text.contains(
                r#"  "agent_end": ["/opt/am bin/am-hook", "hook-event", "--agent", "omp", "--worktree", "", "--event", "agent_end"],"#
            ),
            "{text}"
        );
        assert!(text.contains("const TIMEOUT_MS = 5000;"));
        for event in OMP.hook_events() {
            assert!(text.contains(&format!("  \"{event}\": [")), "{event} has an argv");
            assert!(text.contains(&format!("send(\"{event}\"")) || text.contains(&format!(": \"{event}\"")), "{event} is sent");
        }
        // Hooks run in order, one at a time, and `ask`'s end goes without its name.
        assert!(text.contains("const next = s.queue.then("), "{text}");
        assert!(text.contains("s.queue = next;"));
        assert!(text.contains("tool_name: endedTool(e.toolName)"));
        assert!(text.contains(r#"new Set(["ask", "plan_review"])"#));
        // Nothing from OMP's package is imported at runtime.
        let imports: Vec<&str> = text.lines().filter(|l| l.starts_with("import ")).collect();
        assert_eq!(
            imports,
            ["import { spawn } from \"node:child_process\";", "import * as fs from \"node:fs\";", "import * as path from \"node:path\";"]
        );
        // The installer finds the binary in it (dangling-binary repair).
        assert_eq!(crate::hooks_installer::hook_programs_in_text(&text).first(), Some(&PathBuf::from("/opt/am bin/am-hook")));

        // Without the native binary it runs `am hook-event`, which names no path.
        let text = render_extension(&ctx(None));
        assert!(text.contains(r#"  "session_start": ["am", "hook-event", "--agent", "omp", "--worktree", "", "--event", "session_start"],"#));
        assert!(crate::hooks_installer::hook_programs_in_text(&text).is_empty());
    }

    /// The payloads `omp.ts` sends, as am-hook reads them.
    fn ev(event: &str, extra: Value) -> crate::status::HookEvent {
        let mut p = json!({"session_id": "s1", "cwd": "/w/repo", "hook_event_name": event});
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        OMP.normalize(&p, Some(event))
    }

    #[test]
    fn an_omp_turn_drives_the_status_rules() {
        let step = |cur, e: &crate::status::HookEvent| map_event_to_status(e, cur, 0);
        let s = step(None, &ev("session_start", json!({})));
        assert_eq!(s, Some(Idle));
        let prompt = ev("before_agent_start", json!({"prompt": "fix it"}));
        assert_eq!((prompt.event.as_str(), prompt.session_id.as_deref(), prompt.cwd.as_deref()), ("UserPromptSubmit", Some("s1"), Some("/w/repo")));
        let s = step(s, &prompt);
        assert_eq!(s, Some(Executing));
        let tool = ev("tool_execution_start", json!({"tool_name": "bash", "tool_input": {"command": "ls"}}));
        assert_eq!((tool.event.as_str(), tool.tool_name.as_deref()), ("PreToolUse", Some("Bash")));
        let s = step(s, &tool);
        let s = step(s, &ev("tool_approval_requested", json!({"tool_name": "bash"})));
        assert_eq!(s, Some(Waiting));
        let s = step(s, &ev("tool_approval_resolved", json!({"tool_name": "bash"})));
        assert_eq!(s, Some(Executing));
        let s = step(s, &ev("tool_execution_end", json!({"tool_name": "bash"})));
        let stop = ev("agent_end", json!({"last_assistant_message": "Fixed."}));
        assert_eq!(stop.last_assistant_message.as_deref(), Some("Fixed."));
        let s = step(s, &stop);
        assert_eq!(s, Some(Done));
        let review = ev("plan_review_requested", json!({"tool_name": "plan_review", "tool_input": {"title": "Plan"}}));
        assert_eq!((review.event.as_str(), review.tool_name.as_deref()), ("PreToolUse", Some("ExitPlanMode")));
        assert_eq!(step(s, &review), Some(Waiting), "a plan review after the turn ended still shows");
        assert_eq!(step(Some(Waiting), &stop), Some(Waiting), "and stays when the turn's Stop comes after it");
        assert_eq!(step(s, &ev("session_shutdown", json!({}))), Some(Idle));
        assert_eq!(ev("session_shutdown", json!({})).event, "SessionEnd");
        assert_eq!(ev("ask", json!({})).event, "ask", "unknown events pass through unmapped");
        // The `ask` tool waits on the user; answering it is working again, and
        // a text-only finish after it is done, not waiting.
        let s = step(Some(Executing), &ev("tool_execution_start", json!({"tool_name": "ask"})));
        assert_eq!(s, Some(Waiting));
        let ask_end = ev("tool_execution_end", json!({}));
        assert_eq!((ask_end.event.as_str(), ask_end.tool_name.as_deref()), ("PostToolUse", None));
        let s = step(s, &ask_end);
        assert_eq!(s, Some(Executing));
        assert_eq!(step(s, &stop), Some(Done));
        assert_eq!(
            step(Some(Waiting), &ev("tool_execution_end", json!({"tool_name": "ask"}))),
            Some(Waiting),
            "why the extension drops the name"
        );

        // A subagent (another session in the same omp process) reports under the owner.
        let sub = ev("subagent_start", json!({"agent_id": "s2", "agent_type": "task"}));
        assert_eq!((sub.event.as_str(), sub.agent_id.as_deref(), sub.agent_type.as_deref()), ("SubagentStart", Some("s2"), Some("task")));
        let sub_tool = ev("tool_execution_start", json!({"agent_id": "s2", "agent_type": "task", "tool_name": "read"}));
        assert!(sub_tool.from_subagent());
        assert_eq!(step(Some(Executing), &sub_tool), None, "a subagent's tool doesn't move the parent");
        let sub_ask = ev("tool_approval_requested", json!({"agent_id": "s2", "agent_type": "task"}));
        assert_eq!(step(Some(Executing), &sub_ask), Some(Waiting));
        assert_eq!(ev("subagent_stop", json!({"agent_id": "s2"})).event, "SubagentStop");
    }

    #[test]
    fn resumes_and_recognises_omp() {
        assert_eq!(OMP.resume_args(Some("0f3c"), false), ["--session", "0f3c"]);
        assert_eq!(OMP.resume_args(None, true), ["--continue"]);
        let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(OMP.matches_process("omp", &argv(&["omp"])));
        assert!(OMP.matches_process("bun", &argv(&["bun", "/u/lib/node_modules/@oh-my-pi/pi-coding-agent/dist/cli.js"])));
        assert!(OMP.matches_process("node", &argv(&["node", "/usr/local/bin/omp", "--continue"])));
        assert!(!OMP.matches_process("pi", &argv(&["pi"])));
        assert!(!OMP.matches_process("node", &argv(&["node", "/u/lib/node_modules/@mariozechner/pi-coding-agent/dist/cli.js"])));
    }
}
