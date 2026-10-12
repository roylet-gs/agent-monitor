//! `~/.agent-monitor/settings.json`, shared with the TS app. Keys and defaults
//! mirror `DEFAULT_SETTINGS` in `src/lib/settings.ts`. Unknown keys are kept in
//! `raw` so a future save never drops what the TS app (or a newer version) wrote.

use serde::{Deserialize, Serialize};

use crate::log::Level;
use crate::paths;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SortKey {
    IsMain,
    Repo,
    LinearTicket,
    LinearProject,
    AgentStatus,
    LastActivity,
    CreatedAt,
    BranchName,
    PrStatus,
    GitDirty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Asc,
    Desc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SortCriterion {
    pub key: SortKey,
    pub direction: Direction,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub ide: String,
    pub default_branch_prefix: String,
    pub default_base_branch: String,
    pub polling_interval_ms: u64,
    pub auto_sync_on_startup: bool,
    pub compact_view: bool,
    pub hide_main_branch: bool,
    pub audio_notifications: bool,
    pub audio_waiting_sound: String,
    pub audio_done_sound: String,
    pub gh_pr_status: bool,
    pub gh_polling_interval_ms: u64,
    pub log_level: String,
    pub linear_enabled: bool,
    pub linear_use_desktop_app: bool,
    pub linear_api_key: String,
    pub linear_polling_interval_ms: u64,
    pub gh_refresh_on_manual: bool,
    pub linear_refresh_on_manual: bool,
    pub linear_auto_nickname: bool,
    pub worktree_sort: Vec<SortCriterion>,
    pub hide_merged_closed_prs: bool,
    pub hide_idle_done_agents: bool,
    pub hide_without_linear_ticket: bool,
    pub show_pr_status: bool,
    pub show_linear_ticket: bool,
    pub show_git_ahead_behind: bool,
    pub show_last_commit: bool,
    pub show_running_processes: bool,
    pub running_process_filter: String,
    pub worktree_limit_enabled: bool,
    pub max_worktrees: u32,
    pub max_log_size_mb: u64,
    pub agent_permission_mode: String,
    pub agent_claude_args: String,
    pub resume_last_session: bool,
    // --- Rust-only additions (ignored by the TS app) ---
    /// Theme name: a built-in (`ansi`, the default, follows the terminal
    /// palette) or a custom `themes/<name>.json`; see `crate::themes`.
    pub theme: String,
    /// Use Nerd Font glyphs instead of plain Unicode.
    pub nerd_fonts: bool,
    /// Context-window bar at the far right of each worktree row.
    pub show_context_window: bool,
    /// Context window the bar is scaled to. A session that uses more than
    /// this is shown against 1M instead (see `crate::context::window_for`).
    pub context_window_tokens: u64,
    #[serde(skip)]
    pub raw: serde_json::Map<String, serde_json::Value>,
}

impl Default for Settings {
    fn default() -> Self {
        use Direction::*;
        use SortKey::*;
        let c = |key, direction, enabled| SortCriterion { key, direction, enabled };
        Settings {
            ide: "cursor".into(),
            default_branch_prefix: "feature/".into(),
            default_base_branch: "main".into(),
            polling_interval_ms: 30_000,
            auto_sync_on_startup: true,
            compact_view: false,
            hide_main_branch: true,
            audio_notifications: false,
            audio_waiting_sound: "Glass".into(),
            audio_done_sound: "Funk".into(),
            gh_pr_status: true,
            gh_polling_interval_ms: 180_000,
            log_level: "info".into(),
            linear_enabled: false,
            linear_use_desktop_app: false,
            linear_api_key: String::new(),
            linear_polling_interval_ms: 180_000,
            gh_refresh_on_manual: true,
            linear_refresh_on_manual: true,
            linear_auto_nickname: true,
            worktree_sort: vec![
                c(IsMain, Asc, true),
                c(LinearTicket, Asc, true),
                c(CreatedAt, Desc, true),
                c(Repo, Asc, false),
                c(AgentStatus, Asc, false),
                c(LastActivity, Desc, false),
                c(LinearProject, Asc, false),
                c(PrStatus, Asc, false),
                c(GitDirty, Asc, false),
                c(BranchName, Asc, false),
            ],
            hide_merged_closed_prs: false,
            hide_idle_done_agents: false,
            hide_without_linear_ticket: false,
            show_pr_status: true,
            show_linear_ticket: true,
            show_git_ahead_behind: true,
            show_last_commit: true,
            show_running_processes: false,
            running_process_filter: String::new(),
            worktree_limit_enabled: false,
            max_worktrees: 5,
            max_log_size_mb: 2,
            agent_permission_mode: "acceptEdits".into(),
            agent_claude_args: String::new(),
            resume_last_session: true,
            theme: "ansi".into(),
            nerd_fonts: false,
            show_context_window: true,
            context_window_tokens: 200_000,
            raw: Default::default(),
        }
    }
}

impl Settings {
    pub fn log_level(&self) -> Level {
        Level::parse(&self.log_level).unwrap_or(Level::Info)
    }

    pub fn parse(json: &str) -> Settings {
        let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(json) else {
            return Settings::default();
        };
        // Parse field by field so one bad value (wrong type) can't reset everything.
        let defaults = serde_json::to_value(Settings::default()).unwrap_or_default();
        let mut merged = defaults.as_object().cloned().unwrap_or_default();
        for (k, v) in &map {
            if !merged.contains_key(k) {
                continue;
            }
            let mut probe = merged.clone();
            probe.insert(k.clone(), v.clone());
            if serde_json::from_value::<Settings>(serde_json::Value::Object(probe)).is_ok() {
                merged.insert(k.clone(), v.clone());
            }
        }
        let mut s: Settings = serde_json::from_value(serde_json::Value::Object(merged)).unwrap_or_default();
        s.raw = map;
        s
    }
}

impl Settings {
    /// Every known key with its typed value, in default key order (the TS
    /// `DEFAULT_SETTINGS` order), followed by any unknown keys from the file.
    /// Mirrors what the TS `loadSettings()` returns (`{...DEFAULTS, ...file}`).
    pub fn to_view(&self) -> serde_json::Map<String, serde_json::Value> {
        let mut view = match serde_json::to_value(self) {
            Ok(serde_json::Value::Object(m)) => m,
            _ => serde_json::Map::new(),
        };
        for (k, v) in &self.raw {
            if !view.contains_key(k) {
                view.insert(k.clone(), v.clone());
            }
        }
        view
    }

    /// What [`save`] writes: the file's own keys in their original order
    /// (unknown ones untouched), with every known key set to its typed value
    /// (new keys appended).
    pub fn to_file_json(&self) -> serde_json::Map<String, serde_json::Value> {
        let mut out = self.raw.clone();
        if let Ok(serde_json::Value::Object(typed)) = serde_json::to_value(self) {
            for (k, v) in typed {
                out.insert(k, v);
            }
        }
        out
    }
}

pub fn load() -> Settings {
    match std::fs::read_to_string(paths::settings_path()) {
        Ok(raw) => Settings::parse(&raw),
        Err(_) => Settings::default(),
    }
}

/// Write `settings.json` the way the TS app does (2-space JSON + trailing
/// newline), merged with `raw` so keys the TS app — or a newer version — wrote
/// are preserved. Written via a temp file + rename so a concurrent reader never
/// sees a half-written file.
pub fn save(settings: &Settings) -> std::io::Result<()> {
    let path = paths::settings_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut text = serde_json::to_string_pretty(&serde_json::Value::Object(settings.to_file_json()))
        .map_err(std::io::Error::other)?;
    text.push('\n');
    let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, &path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_keys_fall_back_to_defaults_and_unknown_keys_are_kept() {
        let s = Settings::parse(r#"{"logLevel":"debug","pollingIntervalMs":5000,"showIdeStatus":true}"#);
        assert_eq!(s.log_level(), Level::Debug);
        assert_eq!(s.polling_interval_ms, 5000);
        assert!(s.hide_main_branch);
        assert_eq!(s.worktree_sort.len(), 10);
        assert!(s.raw.contains_key("showIdeStatus"));
    }

    #[test]
    fn one_bad_value_does_not_reset_the_rest() {
        let s = Settings::parse(r#"{"pollingIntervalMs":"fast","ide":"vscode"}"#);
        assert_eq!(s.polling_interval_ms, 30_000);
        assert_eq!(s.ide, "vscode");
    }

    #[test]
    fn sort_criteria_round_trip_ts_shape() {
        let s = Settings::parse(r#"{"worktreeSort":[{"key":"linearProject","direction":"desc","enabled":true}]}"#);
        assert_eq!(s.worktree_sort, vec![SortCriterion { key: SortKey::LinearProject, direction: Direction::Desc, enabled: true }]);
    }

    #[test]
    fn file_json_keeps_unknown_keys_and_their_order() {
        let mut s = Settings::parse(r#"{"setupCompleted":true,"ide":"vscode","zeta":1}"#);
        s.ide = "cursor".into();
        let out = s.to_file_json();
        let keys: Vec<&str> = out.keys().map(String::as_str).take(3).collect();
        assert_eq!(keys, ["setupCompleted", "ide", "zeta"]);
        assert_eq!(out["ide"], "cursor");
        assert_eq!(out["setupCompleted"], true);
        assert!(out.contains_key("worktreeSort"));
    }

    #[test]
    fn view_is_defaults_order_then_extras() {
        let s = Settings::parse(r#"{"zeta":1,"ide":"vscode"}"#);
        let view = s.to_view();
        assert_eq!(view.keys().next().map(String::as_str), Some("ide"));
        assert_eq!(view.keys().next_back().map(String::as_str), Some("zeta"));
        assert_eq!(view["ide"], "vscode");
    }

    #[test]
    fn garbage_file_gives_defaults() {
        assert_eq!(Settings::parse("not json").polling_interval_ms, 30_000);
    }
}
