//! Kiro CLI (AWS, `kiro-cli`) as a built-in plugin: the manifest `kiro.json`
//! plus a little Rust for where the hooks live and when Kiro CLI is installed.
//!
//! Kiro keeps hooks inside *agent configuration files*, so am owns a whole
//! agent, `~/.kiro/agents/agent-monitor.json` (`$KIRO_HOME/agents/` when that
//! is set): `{"name", "description", "tools": ["*"], "hooks": {event:
//! [{"command", "timeout_ms"}]}}` with camelCase events (`agentSpawn`,
//! `userPromptSubmit`, `preToolUse`, `postToolUse`, `stop`). This is the
//! 2.x object form, which Kiro v3 still accepts. `tools: ["*"]` matters: a
//! custom agent without `tools` gets none, so the tool hooks would never fire.
//! `includeMcpJson: true` matters too: a custom agent only gets the user's
//! MCP servers (`~/.kiro/settings/mcp.json`, the workspace's
//! `.kiro/settings/mcp.json`) when it opts in, unlike Kiro's default agent.
//! It is Kiro's documented key (CLI 2.x and v3); Amazon Q CLI's older
//! `useLegacyMcpJson` is not used. Like every file default, both are only
//! added when missing.
//!
//! **Presence:** the Kiro IDE creates `~/.kiro` too, so the directory alone
//! doesn't mean Kiro CLI is installed: am also wants `kiro-cli` on `$PATH`
//! (or in `~/.local/bin`, where its installer puts it).
//!
//! **Quirk:** Kiro runs an agent's hooks only while that agent is in use, so
//! a session is tracked only when started as `kiro-cli chat --agent
//! agent-monitor` or with `chat.defaultAgent` set to it. Starting or resuming
//! Kiro from am therefore always passes `--agent agent-monitor`.
//!
//! Payloads carry Claude's keys (`hook_event_name`, `session_id`, `cwd`,
//! `tool_name`, `tool_input`); there is no project-dir variable, so am-hook
//! uses the payload's `cwd`. Kiro has no permission or notification hook, so
//! a session never shows `waiting`, and no subagent ids, so no `delegating`.
//! Exit 2 from a `preToolUse` hook blocks the tool (stderr goes to the
//! agent), any other non-zero exit shows stderr as a warning, and stdout is
//! captured: am-hook always exits 0 and prints nothing. Resume discovery uses the sessions am's hooks
//! recorded (Kiro keeps its conversations in a SQLite store).

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::embedded::Embedded;
use super::home_join;
use super::manifest::Manifest;

/// The Kiro agent am's hooks live in.
pub const AGENT_NAME: &str = "agent-monitor";

/// Starting or resuming Kiro always names the `agent-monitor` agent: the
/// manifest's `resume` (`start` included) carries `chat --agent agent-monitor`.
pub static KIRO: Embedded = Embedded::new(|| Manifest::builtin(include_str!("kiro.json")), None)
    .with_hook_file(hook_file)
    .with_presence(|| is_present(&kiro_home(std::env::var_os("KIRO_HOME")), std::env::var_os("PATH"), &home_join(".local/bin")));

fn hook_file() -> (PathBuf, &'static str) {
    (kiro_home(std::env::var_os("KIRO_HOME")).join("agents").join(format!("{AGENT_NAME}.json")), "hooks")
}

/// Kiro CLI is installed: its home exists (`$KIRO_HOME` or `~/.kiro`) and
/// `kiro-cli` is on `path` or in `local_bin`. The Kiro IDE makes `~/.kiro`
/// as well, so the directory alone would install hooks nothing runs.
fn is_present(home: &Path, path: Option<OsString>, local_bin: &Path) -> bool {
    let has = |dir: &Path| crate::hooks_installer::is_executable(&dir.join("kiro-cli"));
    home.is_dir()
        && (has(local_bin) || path.is_some_and(|p| std::env::split_paths(&p).any(|d| !d.as_os_str().is_empty() && has(&d))))
}

/// `$KIRO_HOME` when set, else `~/.kiro`.
fn kiro_home(var: Option<OsString>) -> PathBuf {
    var.filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(|| home_join(".kiro"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::Harness;
    use crate::status::{Status::*, map_event_to_status};
    use serde_json::{Value, json};

    fn payload(event: &str) -> Value {
        json!({"hook_event_name": event, "cwd": "/w/repo", "session_id": "abc123-def456"})
    }

    #[test]
    fn a_kiro_turn_drives_the_status_rules() {
        let step = |cur, p: Value, ev: &str| map_event_to_status(&KIRO.normalize(&p, Some(ev)), cur, 0);
        let e = KIRO.normalize(&payload("agentSpawn"), None);
        assert_eq!((e.event.as_str(), e.session_id.as_deref(), e.cwd.as_deref()), ("SessionStart", Some("abc123-def456"), Some("/w/repo")));
        let s = step(None, payload("userPromptSubmit"), "userPromptSubmit");
        assert_eq!(s, Some(Executing));
        let mut tool = payload("preToolUse");
        tool["tool_name"] = "execute_bash".into();
        tool["tool_input"] = json!({"command": "cargo test"});
        let e = KIRO.normalize(&tool, Some("preToolUse"));
        assert_eq!((e.event.as_str(), e.tool_name.as_deref()), ("PreToolUse", Some("Bash")));
        let s = step(s, tool, "preToolUse").or(s);
        assert_eq!(s, Some(Executing));
        assert_eq!(step(s, payload("stop"), "stop"), Some(Done));
        // Kiro v3 names its events in PascalCase in the payload; the
        // `--event` flag from the installed command still decides.
        assert_eq!(KIRO.normalize(&payload("PreToolUse"), Some("preToolUse")).event, "PreToolUse");
        assert_eq!(KIRO.normalize(&payload("Stop"), Some("stop")).event, "Stop");
    }

    #[test]
    fn hook_entries_are_flat_with_a_millisecond_timeout() {
        let e = KIRO.hook_entry("stop", "\"/bin/am-hook\" hook-event", true);
        assert_eq!(e, json!({"command": "\"/bin/am-hook\" hook-event --agent kiro --worktree \"\" --event stop", "timeout_ms": 5000}));
        assert_eq!(KIRO.hook_events(), ["agentSpawn", "userPromptSubmit", "preToolUse", "postToolUse", "stop"]);
        assert_eq!(KIRO.hooks_key(), "hooks");
        let d = KIRO.hook_file_defaults();
        assert_eq!((d.get("name"), d.get("tools")), (Some(&json!("agent-monitor")), Some(&json!(["*"]))));
        // A custom agent gets the user's MCP servers only when it opts in.
        assert_eq!(d.get("includeMcpJson"), Some(&json!(true)));
        assert!(d.get("description").is_some_and(Value::is_string));
        assert_eq!(KIRO.program(), "kiro-cli");
        assert!(KIRO.notes().unwrap().contains("--agent agent-monitor"));
    }

    #[test]
    fn the_hook_file_is_an_agent_under_kiro_home() {
        assert_eq!(kiro_home(Some("/opt/kiro".into())), PathBuf::from("/opt/kiro"));
        assert_eq!(kiro_home(Some("".into())), home_join(".kiro"));
        assert_eq!(kiro_home(None), home_join(".kiro"));
        assert!(KIRO.settings_path().ends_with("agents/agent-monitor.json"));
    }

    #[test]
    fn presence_needs_kiro_cli_not_just_the_kiro_dir() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join(".kiro");
        let bin = tmp.path().join("bin");
        let local = tmp.path().join("local");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&local).unwrap();
        let path = || Some(OsString::from(format!("/nonexistent:{}", bin.display())));
        // No ~/.kiro: not present.
        assert!(!is_present(&home, path(), &local));
        // The Kiro IDE's ~/.kiro without kiro-cli: not present.
        std::fs::create_dir_all(&home).unwrap();
        assert!(!is_present(&home, path(), &local));
        let exe = |p: &Path| {
            std::fs::write(p, "#!/bin/sh\n").unwrap();
            std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
        };
        exe(&local.join("kiro-cli"));
        assert!(is_present(&home, None, &local));
        std::fs::remove_file(local.join("kiro-cli")).unwrap();
        exe(&bin.join("kiro-cli"));
        assert!(is_present(&home, path(), &local));
        assert!(!is_present(&home, None, &local));
    }

    #[test]
    fn starting_and_resuming_keep_the_agent() {
        assert_eq!(KIRO.resume_args(Some("abc"), false), ["chat", "--agent", "agent-monitor", "--resume-id", "abc"]);
        assert_eq!(KIRO.resume_args(None, true), ["chat", "--agent", "agent-monitor", "--resume"]);
        assert_eq!(KIRO.resume_args(None, false), ["chat", "--agent", "agent-monitor"]);
    }

    #[test]
    fn kiro_processes_are_recognised() {
        let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(KIRO.matches_process("kiro-cli", &argv(&["kiro-cli", "chat"])));
        assert!(KIRO.matches_process("kiro-cli-chat", &argv(&["kiro-cli-chat", "--agent", "agent-monitor"])));
        assert!(KIRO.matches_process("2.5.0", &argv(&["/Users/me/.local/bin/kiro-cli", "chat"])));
        assert!(!KIRO.matches_process("kiro", &argv(&["/Applications/Kiro.app/Contents/MacOS/Kiro"])));
        assert!(!KIRO.matches_process("cargo", &argv(&["cargo", "test"])));
    }
}
