//! Qoder CLI (`qodercli`) as a built-in plugin, entirely the manifest `qoder.json`.
//!
//! Qoder's hooks follow Claude Code's layout, event names and payload keys
//! (`~/.qoder/settings.json`, nested `hooks` → matcher groups → `{type,
//! command, timeout}`, `$QODER_PROJECT_DIR`), including subagent, permission
//! and `StopFailure` events, and its tool names are Claude's (`Bash`, `Edit`…),
//! so nothing is renamed. Quirks:
//! - `timeout` is in seconds (Qoder's docs; default 600). cmux writes `5000`
//!   there as if it were milliseconds, which Qoder reads as 83 minutes.
//! - The CLI also honours `$QODER_CONFIG_DIR`; am installs into the default
//!   `~/.qoder` only.
//! - The npm package ships two launchers, `qoder` (a dispatcher) and
//!   `qodercli`; only `qodercli` is matched by name, since `qoder` may also
//!   be the Qoder IDE's shell command. The npm build runs under `node` and is
//!   found through its package path.
//! - The Qoder IDE and its JetBrains plugin read the same
//!   `~/.qoder/settings.json` hooks, but don't set `$QODER_PROJECT_DIR`, so
//!   their agent turns arrive as `--worktree ""` and am-hook falls back to the
//!   payload `cwd` (an IDE payload without `cwd` can't be placed and is
//!   dropped). They're recorded as `qoder` sessions, and resume offers
//!   `qodercli --resume <id>`, which may not find an IDE session.
//! - The CLI reloads hooks live; the Qoder IDE reads them at startup, so it
//!   needs a restart after installing.
//! - Resume is `qodercli --resume <id>` (`-r`), continue `-c`. No session
//!   store layout is documented, so resume discovery uses the sessions am's
//!   hooks recorded.

use super::embedded::Embedded;
use super::manifest::Manifest;

pub static QODER: Embedded = Embedded::new(|| Manifest::builtin(include_str!("qoder.json")), None);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::Harness;
    use crate::status::{Status::*, map_event_to_status};
    use serde_json::{Value, json};

    #[test]
    fn a_qoder_turn_drives_the_status_rules() {
        let step = |cur, p: Value| map_event_to_status(&QODER.normalize(&p, None), cur, 0);
        let s = step(None, json!({"hook_event_name":"UserPromptSubmit","session_id":"k1","cwd":"/w","prompt":"hi"}));
        assert_eq!(s, Some(Executing));
        let e = QODER.normalize(&json!({"hook_event_name":"PreToolUse","session_id":"k1","tool_name":"Bash","tool_input":{"command":"npm test"},"tool_use_id":"t1"}), None);
        assert_eq!((e.tool_name.as_deref(), e.session_id.as_deref()), (Some("Bash"), Some("k1")));
        assert_eq!(step(s, json!({"hook_event_name":"PermissionRequest","tool_name":"Bash","tool_input":{"command":"rm -rf x"}})), Some(Waiting));
        assert_eq!(step(s, json!({"hook_event_name":"Stop","stop_hook_active":false,"last_assistant_message":"done"})), Some(Done));
    }

    #[test]
    fn hook_entries_resume_and_process() {
        let e = QODER.hook_entry("Stop", "\"/bin/am-hook\" hook-event", true);
        let h = &e["hooks"][0];
        assert_eq!((h["type"].as_str(), h["timeout"].as_u64()), (Some("command"), Some(5)));
        assert_eq!(h["command"], "\"/bin/am-hook\" hook-event --agent qoder --worktree \"$QODER_PROJECT_DIR\" --event Stop");
        assert_eq!(QODER.program(), "qodercli");
        assert_eq!(QODER.resume_args(Some("k1"), false), ["--resume", "k1"]);
        assert_eq!(QODER.resume_args(None, true), ["-c"]);
        assert!(QODER.matches_process("qodercli", &["qodercli".into()]));
        assert!(QODER.matches_process("node", &["node".into(), "/usr/lib/node_modules/@qoder-ai/qodercli/bundle/qodercli.js".into()]));
        assert!(!QODER.matches_process("qwen", &["qwen".into()]));
    }
}
