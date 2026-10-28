//! Kimi Code CLI (Moonshot, `kimi`) as a built-in plugin: the manifest
//! `kimi.json` plus a TOML hook block.
//!
//! Kimi's hooks are `[[hooks]]` tables (`event`, `command`, `timeout` in
//! seconds; `matcher` optional, any other key fails Kimi's config load) in
//! its `config.toml`, so am writes them as a [`TextBlock`] between
//! `# agent-monitor hooks kimi begin/end` lines and leaves the rest of the
//! file alone. The file is Kimi Code CLI's
//! `${KIMI_CODE_HOME:-~/.kimi-code}/config.toml`, and only its directory
//! makes Kimi present. The legacy Python Kimi CLI (≤ 1.49,
//! `${KIMI_SHARE_DIR:-~/.kimi}/config.toml`) is deliberately not supported:
//! it validates `[[hooks]]` events against a closed list without
//! `PermissionRequest`, `PermissionResult` or `Interrupt` (so the block would
//! stop it from starting), and it rewrites its config on `/login`/`/model`,
//! dropping the block's marker comments.
//! A `hooks = [...]` inline array in the user's file would clash with the
//! appended `[[hooks]]` tables; Kimi's docs only use the table form.
//!
//! Event names and payload keys are Claude Code's (`hook_event_name`,
//! `session_id`, `cwd`, `tool_name`, `tool_input`) and so are the tool names
//! (`Bash`, `Edit`, `AskUserQuestion`, `ExitPlanMode`…). There is no
//! project-dir variable: hooks run in the session's directory and the
//! command passes `--worktree ""`, so am-hook uses the payload's `cwd`.
//! Kimi-specific events: `PermissionRequest` (waiting) has a matching
//! `PermissionResult`, mapped to `PostToolUse` so the row goes back to
//! executing; `PostToolUseFailure` is a `PostToolUse`; a cancelled turn
//! (Esc) fires `Interrupt` instead of `Stop`, mapped to `StopFailure`
//! (idle). `Notification` only reports background tasks
//! (`task.completed`…), never an approval, so it isn't hooked.
//! `SubagentStart`/`SubagentStop` carry a name but no id, so subagents
//! aren't tracked. Resume is `kimi --session <id>` / `--continue`
//! (`--resume` is a hidden alias); discovery uses the sessions am's hooks
//! recorded.

use std::path::{Path, PathBuf};

use super::embedded::Embedded;
use super::manifest::{Manifest, expand_home};
use crate::hooks_installer::{HookContext, HookInstall, TextBlock, toml_string};

pub static KIMI: Embedded = Embedded::new(|| Manifest::builtin(include_str!("kimi.json")), None)
    .with_hook_file(hook_file)
    .with_presence(|| hook_file().0.parent().is_some_and(Path::is_dir))
    .with_hook_install(|| HookInstall::TextBlock(TextBlock { comment: "#", render }));

/// One `[[hooks]]` table per event, separated by blank lines.
fn render(ctx: &HookContext) -> String {
    ctx.hooks
        .iter()
        .map(|h| format!("[[hooks]]\nevent = {}\ncommand = {}\ntimeout = {}\n", toml_string(&h.event), toml_string(&h.command), h.timeout_secs()))
        .collect::<Vec<_>>()
        .join("\n")
}

/// `config.toml` in `$KIMI_CODE_HOME`, else `~/.kimi-code`.
fn hook_file() -> (PathBuf, &'static str) {
    (home_dir(std::env::var("KIMI_CODE_HOME").ok().as_deref()).join("config.toml"), "")
}

fn home_dir(var: Option<&str>) -> PathBuf {
    expand_home(var.map(str::trim).filter(|v| !v.is_empty()).unwrap_or("~/.kimi-code"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::Harness;
    use crate::status::{Status::*, map_event_to_status};
    use serde_json::{Value, json};
    use std::path::Path;

    fn step(cur: Option<crate::status::Status>, p: Value) -> Option<crate::status::Status> {
        map_event_to_status(&KIMI.normalize(&p, None), cur, 0)
    }

    #[test]
    fn a_kimi_turn_drives_the_status_rules() {
        let base = |e: &str| json!({"hook_event_name": e, "session_id": "k1", "session_title": "t", "client_type": "kimi_code_cli", "cwd": "/w"});
        let s = step(None, base("SessionStart"));
        assert_eq!(s, Some(Idle));
        let s = step(s, json!({"hook_event_name":"UserPromptSubmit","session_id":"k1","cwd":"/w","prompt":"hi","is_steer":false}));
        assert_eq!(s, Some(Executing));
        let e = KIMI.normalize(&json!({"hook_event_name":"PreToolUse","session_id":"k1","cwd":"/w","tool_name":"Bash","tool_input":{"command":"ls"},"tool_call_id":"c1"}), None);
        assert_eq!((e.session_id.as_deref(), e.cwd.as_deref(), e.tool_name.as_deref()), (Some("k1"), Some("/w"), Some("Bash")));
        let s = step(s, json!({"hook_event_name":"PermissionRequest","tool_name":"Bash"}));
        assert_eq!(s, Some(Waiting));
        let s = step(s, json!({"hook_event_name":"PermissionResult","tool_name":"Bash"}));
        assert_eq!(s, Some(Executing), "an answered approval resumes the turn");
        let s = step(s, json!({"hook_event_name":"PostToolUseFailure","tool_name":"Bash","error":{}}));
        assert_eq!(s, Some(Executing));
        assert_eq!(step(s, json!({"hook_event_name":"Stop","stop_hook_active":false})), Some(Done));
        assert_eq!(step(s, json!({"hook_event_name":"Interrupt","reason":"cancelled"})), Some(Idle));
        assert_eq!(step(Some(Waiting), json!({"hook_event_name":"Interrupt","reason":"cancelled"})), Some(Idle), "Esc at a prompt");
        assert_eq!(step(s, json!({"hook_event_name":"StopFailure","error_type":"x"})), Some(Idle));
        assert_eq!(step(s, json!({"hook_event_name":"PreToolUse","tool_name":"AskUserQuestion"})), Some(Waiting));
        assert_eq!(step(Some(Done), base("SessionEnd")), Some(Idle));
    }

    #[test]
    fn renders_toml_hook_tables() {
        let ctx = HookContext::new(&KIMI, Some(Path::new("/b/am-hook")));
        let out = render(&ctx);
        let first = "[[hooks]]\nevent = \"SessionStart\"\ncommand = \"\\\"/b/am-hook\\\" hook-event --agent kimi --worktree \\\"\\\" --event SessionStart\"\ntimeout = 5\n";
        assert!(out.starts_with(&format!("{first}\n[[hooks]]\nevent = \"UserPromptSubmit\"\n")), "{out}");
        assert!(out.ends_with("--event SessionEnd\"\ntimeout = 5\n"), "{out}");
        assert_eq!(out.matches("[[hooks]]").count(), 11);
        // Only the four keys Kimi accepts in a [[hooks]] table.
        for line in out.lines().filter(|l| !l.is_empty() && *l != "[[hooks]]") {
            let key = line.split(" = ").next().unwrap();
            assert!(["event", "command", "timeout"].contains(&key), "{line}");
        }
        let fallback = render(&HookContext::new(&KIMI, None));
        assert!(fallback.contains("command = \"cat | am hook-event --agent kimi --worktree \\\"\\\" --event Stop\""), "{fallback}");
    }

    #[test]
    fn the_config_is_kimi_codes_never_the_legacy_one() {
        assert_eq!(home_dir(Some("/k")), Path::new("/k"));
        assert_eq!(home_dir(Some("  ")), expand_home("~/.kimi-code"), "an empty $KIMI_CODE_HOME is unset");
        assert_eq!(home_dir(None), expand_home("~/.kimi-code"));
        assert!(!home_dir(None).ends_with(".kimi"));
    }

    #[test]
    fn resume_and_processes() {
        assert_eq!(KIMI.resume_args(Some("k1"), false), ["--session", "k1"]);
        assert_eq!(KIMI.resume_args(None, true), ["--continue"]);
        assert!(KIMI.matches_process("kimi", &["kimi".into()]));
        assert!(KIMI.matches_process("node", &["node".into(), "/usr/local/bin/kimi".into()]));
        assert!(KIMI.matches_process("node", &["node".into(), "/usr/lib/node_modules/@moonshot-ai/kimi-code/dist/main.mjs".into()]));
        assert!(!KIMI.matches_process("node", &["node".into(), "/srv/app.js".into()]));
        assert!(matches!(KIMI.hook_install(), HookInstall::TextBlock(_)));
    }
}
