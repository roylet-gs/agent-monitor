//! Factory Droid as a built-in plugin: the manifest `droid.json`.
//!
//! Droid's hooks use Claude Code's entry shape, event names and payload keys,
//! with `$FACTORY_PROJECT_DIR`. They live in `~/.factory/hooks.json`, keyed by
//! event at the top level; when that file doesn't exist Droid reads the
//! `hooks` key of `~/.factory/settings.json` instead, so am writes to
//! whichever one Droid will read. Tool names (`Execute`, `Create`…) are mapped
//! onto Claude's. A cancelled turn sends an `idle_prompt` notification rather
//! than `Stop`. Resume discovery uses the sessions am's hooks recorded.

use std::path::PathBuf;

use super::embedded::Embedded;
use super::home_join;
use super::manifest::Manifest;

pub static DROID: Embedded =
    Embedded::new(|| Manifest::builtin(include_str!("droid.json")), None).with_hook_file(hook_file);

fn hook_file() -> (PathBuf, &'static str) {
    pick_hook_file(home_join(".factory/hooks.json"), home_join(".factory/settings.json"))
}

fn pick_hook_file(hooks_json: PathBuf, settings_json: PathBuf) -> (PathBuf, &'static str) {
    if !hooks_json.exists() && settings_json.exists() { (settings_json, "hooks") } else { (hooks_json, "") }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::Harness;
    use crate::status::{Status::*, map_event_to_status};
    use serde_json::{Value, json};

    #[test]
    fn a_droid_turn_drives_the_status_rules() {
        let step = |cur, p: Value| map_event_to_status(&DROID.normalize(&p, None), cur, 0);
        let s = step(None, json!({"hook_event_name":"UserPromptSubmit","session_id":"d1","cwd":"/w"}));
        assert_eq!(s, Some(Executing));
        let e = DROID.normalize(&json!({"hook_event_name":"PreToolUse","tool_name":"Execute","tool_input":{"command":"ls"}}), None);
        assert_eq!(e.tool_name.as_deref(), Some("Bash"));
        assert_eq!(step(s, json!({"hook_event_name":"Notification","notification_type":"permission_prompt"})), Some(Waiting));
        assert_eq!(step(s, json!({"hook_event_name":"Stop"})), Some(Done));
    }

    #[test]
    fn hooks_go_where_droid_reads_them() {
        let dir = tempfile::tempdir().unwrap();
        let (hj, sj) = (dir.path().join("hooks.json"), dir.path().join("settings.json"));
        assert_eq!(pick_hook_file(hj.clone(), sj.clone()), (hj.clone(), ""));
        std::fs::write(&sj, "{}").unwrap();
        assert_eq!(pick_hook_file(hj.clone(), sj.clone()), (sj.clone(), "hooks"));
        std::fs::write(&hj, "{}").unwrap();
        assert_eq!(pick_hook_file(hj.clone(), sj), (hj, ""));
        let e = DROID.hook_entry("Stop", "\"/bin/am-hook\" hook-event", true);
        assert_eq!(e["hooks"][0]["command"], "\"/bin/am-hook\" hook-event --agent droid --worktree \"$FACTORY_PROJECT_DIR\" --event Stop");
        assert_eq!(DROID.resume_args(Some("d1"), false), ["--resume", "d1"]);
    }
}
