//! `~/.agent-monitor/settings.json`, shared with the TS app. Keys and defaults
//! mirror `DEFAULT_SETTINGS` in `src/lib/settings.ts`. Unknown keys are kept in
//! `raw` so a future save never drops what the TS app (or a newer version) wrote.

use serde::{Deserialize, Serialize};

use crate::log::Level;
pub use crate::review_groups::ReviewGroup;
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
    /// Command line for `ide: "custom"` (see `launch::custom_command`).
    pub ide_command: String,
    pub default_branch_prefix: String,
    pub default_base_branch: String,
    pub polling_interval_ms: u64,
    pub auto_sync_on_startup: bool,
    /// Dense layout for small panes (1.x's key, reused): panes keep only
    /// their top line, no blank rows, fewer columns, side panes open below.
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
    /// Switch for the dashboard's picked worktree filters (`F`): off ignores
    /// the `worktreeFilter*` keys. The `hide*` base filters always apply.
    pub worktree_filters_enabled: bool,
    /// Only rows whose name, branch or ticket contains this (any case).
    pub worktree_filter_text: String,
    /// `WORKTREE_STATUS_FILTERS`: which agent statuses the list shows.
    pub worktree_filter_status: String,
    /// `WORKTREE_PR_FILTERS`: which PR states the list shows.
    pub worktree_filter_pr: String,
    /// `WORKTREE_GIT_FILTERS`: local changes or not.
    pub worktree_filter_git: String,
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
    /// Agent CLI (`harness::HarnessKind` id) that Enter starts in a worktree with
    /// no session history yet.
    pub default_agent: String,
    /// Harness plugin ids switched off (`harness::is_enabled`): no hooks, no
    /// session discovery, not installed by default.
    pub disabled_agents: Vec<String>,
    // --- Rust-only additions (ignored by the TS app) ---
    /// Theme name: a built-in (`ansi`, the default, follows the terminal
    /// palette) or a custom `themes/<name>.json`; see `crate::themes`.
    pub theme: String,
    /// Use Nerd Font glyphs instead of plain Unicode.
    pub nerd_fonts: bool,
    /// Mouse capture in the TUI (click, double-click, wheel). Off restores
    /// the terminal's own click-drag text selection.
    pub mouse_enabled: bool,
    /// Context-window bar at the far right of each worktree row.
    pub show_context_window: bool,
    /// Context window the bar is scaled to. A session that uses more than
    /// this is shown against the setting doubled until it fits (see
    /// `crate::context::window_for`).
    pub context_window_tokens: u64,
    /// Today's token usage at the bottom right of the worktree list.
    pub show_daily_tokens: bool,
    /// What that total counts: `"all"` of today's usage, or only the usage of
    /// the rows currently `"visible"`. Unknown values count all.
    pub daily_tokens_scope: String,
    /// How that total is shown (see [`DailyTokensMode`]); unknown values show
    /// raw tokens.
    pub daily_tokens_mode: String,
    /// Check GitHub Releases for a newer `am` and show it in the dashboard.
    pub check_for_updates: bool,
    /// Inside cmux: show each worktree's PR, ticket and context use on its
    /// cmux workspace's sidebar row, and post new review requests as cmux
    /// notifications (`crate::cmux`). Does nothing outside cmux.
    pub cmux_sync: bool,
    /// What `am` opens to (see [`AppMode`]): `full` (the dashboard) or
    /// `reviews` (only the review-requests page). Unknown values are `full`.
    pub app_mode: String,
    /// Where a repo's startup script runs after creating a worktree: `pane`
    /// (default; output streams into the TUI's jobs pane) or `window` (a new
    /// terminal window, falling back to the pane when none can be opened).
    pub startup_script_mode: String,
    /// Dashboard key overrides: TUI command id (`newWorktree`, `moveDown`…) →
    /// key (`N`, `ctrl-d`, `enter`, `none`). Only changed keys are stored;
    /// the TUI's `keymap` resolves and validates them.
    pub key_bindings: std::collections::BTreeMap<String, String>,
    /// Fetch the open PRs waiting on your review (the `v` page and the
    /// `N to review` badge). Needs `ghPrStatus`.
    pub review_requests_enabled: bool,
    /// Whose review requests count: `all` (you or a team you're on),
    /// `direct` (you by name) or `teams` (only your teams). Unknown = `all`.
    pub review_requests_scope: String,
    /// Legacy (2.1.x): leave drafts out. Kept for 1.x/2.x round-trips; only
    /// read to seed `reviewDefaultGroup` when the file has none.
    pub review_hide_drafts: bool,
    /// Legacy: leave out PRs you've reviewed (seeds the default group).
    pub review_hide_reviewed: bool,
    /// Legacy: only these tracked repos (seeds the default group).
    pub review_include_repos: String,
    /// Legacy: never these tracked repos (seeds the default group).
    pub review_exclude_repos: String,
    /// Legacy: extra GitHub search qualifiers (seeds the default group).
    pub review_search_qualifiers: String,
    pub review_polling_interval_ms: u64,
    /// Legacy: the review page's order (seeds the default group's `sort`).
    pub review_sort: String,
    /// The review page's groups, top to bottom: each PR lands in the first
    /// one whose rules it matches (see [`crate::review_groups`]).
    pub review_groups: Vec<ReviewGroup>,
    /// Catches what no group did; what it doesn't match is hidden. Without a
    /// name it keeps its `sort`'s sections. A file without one gets it from
    /// the legacy `review*` filter keys ([`Settings::parse`]).
    pub review_default_group: ReviewGroup,
    /// The review badge also shows every open PR requesting you or your
    /// teams on GitHub, ignoring the filters (`· 60 on GitHub`).
    pub review_badge_total: bool,
    #[serde(skip)]
    pub raw: serde_json::Map<String, serde_json::Value>,
}

impl Default for Settings {
    fn default() -> Self {
        use Direction::*;
        use SortKey::*;
        let c = |key, direction, enabled| SortCriterion { key, direction, enabled };
        Settings {
            ide: "vscode".into(),
            ide_command: String::new(),
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
            worktree_filters_enabled: true,
            worktree_filter_text: String::new(),
            worktree_filter_status: "any".into(),
            worktree_filter_pr: "any".into(),
            worktree_filter_git: "any".into(),
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
            default_agent: "claude".into(),
            disabled_agents: Vec::new(),
            theme: "ansi".into(),
            nerd_fonts: false,
            mouse_enabled: true,
            show_context_window: true,
            context_window_tokens: 1_000_000,
            show_daily_tokens: true,
            daily_tokens_scope: "all".into(),
            daily_tokens_mode: "tokens".into(),
            check_for_updates: true,
            cmux_sync: true,
            app_mode: "full".into(),
            startup_script_mode: "pane".into(),
            key_bindings: Default::default(),
            review_requests_enabled: true,
            review_requests_scope: "all".into(),
            review_hide_drafts: true,
            review_hide_reviewed: true,
            review_include_repos: String::new(),
            review_exclude_repos: String::new(),
            review_search_qualifiers: String::new(),
            review_polling_interval_ms: 120_000,
            review_sort: "direct".into(),
            review_groups: Vec::new(),
            review_default_group: ReviewGroup::default(),
            review_badge_total: false,
            raw: Default::default(),
        }
    }
}

impl Settings {
    pub fn default_agent(&self) -> crate::harness::HarnessKind {
        crate::harness::HarnessKind::from_db(Some(&self.default_agent))
    }

    /// Run startup scripts in a new terminal window rather than the jobs pane.
    pub fn startup_script_in_window(&self) -> bool {
        self.startup_script_mode == "window"
    }

    /// Today's token total counts only the visible rows.
    pub fn daily_tokens_visible_only(&self) -> bool {
        self.daily_tokens_scope == "visible"
    }

    pub fn daily_tokens_mode(&self) -> DailyTokensMode {
        DailyTokensMode::parse(&self.daily_tokens_mode).unwrap_or(DailyTokensMode::Tokens)
    }

    pub fn app_mode(&self) -> AppMode {
        AppMode::parse(&self.app_mode).unwrap_or(AppMode::Full)
    }

    /// Review-only mode: `am` opens to the review page alone, and only what
    /// that page needs is fetched, shown in Settings and asked in setup.
    pub fn reviews_mode(&self) -> bool {
        self.app_mode() == AppMode::Reviews
    }

    /// Review requests are fetched at all (they ride on the PR integration).
    pub fn review_requests_on(&self) -> bool {
        self.gh_pr_status && self.review_requests_enabled
    }

    pub fn review_scope(&self) -> ReviewScope {
        ReviewScope::parse(&self.review_requests_scope).unwrap_or(ReviewScope::All)
    }

    pub fn review_sort(&self) -> ReviewSort {
        ReviewSort::parse(&self.review_sort).unwrap_or(ReviewSort::Direct)
    }

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
        const LEGACY: [&str; 6] = [
            "reviewHideDrafts",
            "reviewHideReviewed",
            "reviewIncludeRepos",
            "reviewExcludeRepos",
            "reviewSearchQualifiers",
            "reviewSort",
        ];
        if !map.contains_key("reviewDefaultGroup") && LEGACY.iter().any(|k| map.contains_key(*k)) {
            s.review_default_group = s.legacy_default_group();
        }
        s.raw = map;
        s
    }

    /// The default review group the 2.1.x filter settings describe, so an
    /// upgrade shows the same page.
    fn legacy_default_group(&self) -> ReviewGroup {
        ReviewGroup {
            sort: self.review_sort().id().into(),
            repos: split_logins(&self.review_include_repos),
            exclude_repos: split_logins(&self.review_exclude_repos),
            draft: if self.review_hide_drafts { "ready" } else { "any" }.into(),
            reviewed: if self.review_hide_reviewed { "no" } else { "any" }.into(),
            query: self.review_search_qualifiers.split_whitespace().collect::<Vec<_>>().join(" "),
            ..ReviewGroup::default()
        }
    }
}

/// A comma/space separated list of GitHub logins, lower-cased, `@` dropped,
/// duplicates removed (first one kept).
pub fn split_logins(raw: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for login in raw.split([',', ' ', '\t', '\n']).map(|s| s.trim().trim_start_matches('@').to_lowercase()) {
        if !login.is_empty() && !out.contains(&login) {
            out.push(login);
        }
    }
    out
}

/// `worktreeFilterStatus` values: every row, rows waiting on you, rows with
/// a working agent (waiting, executing, planning, delegating), or rows whose
/// agent is idle, done or absent.
pub const WORKTREE_STATUS_FILTERS: [&str; 4] = ["any", "waiting", "active", "idle"];
/// `worktreeFilterPr` values: every row, an open PR, no PR, CI failing.
pub const WORKTREE_PR_FILTERS: [&str; 4] = ["any", "open", "none", "failing"];
/// `worktreeFilterGit` values: every row, uncommitted or unpushed work, none.
pub const WORKTREE_GIT_FILTERS: [&str; 3] = ["any", "changes", "clean"];

/// What today's token total shows (`dailyTokensMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DailyTokensMode {
    /// Every billed token, unweighted (cache reads dominate).
    Tokens,
    /// Input-equivalent tokens: each class weighted by its price ratio.
    Weighted,
    /// Estimated US dollars at API list prices.
    Cost,
    /// Raw tokens per class (input, cache write, cache read, output).
    Breakdown,
}

impl DailyTokensMode {
    pub const IDS: [&str; 4] = ["tokens", "weighted", "cost", "breakdown"];

    pub fn parse(s: &str) -> Option<DailyTokensMode> {
        Some(match s {
            "tokens" => DailyTokensMode::Tokens,
            "weighted" => DailyTokensMode::Weighted,
            "cost" => DailyTokensMode::Cost,
            "breakdown" => DailyTokensMode::Breakdown,
            _ => return None,
        })
    }
}

/// What `am` opens to (`appMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    /// The worktree dashboard, with the review page on `v`.
    Full,
    /// Only the review-requests page.
    Reviews,
}

impl AppMode {
    pub const IDS: [&str; 2] = ["full", "reviews"];

    pub fn parse(s: &str) -> Option<AppMode> {
        Some(match s {
            "full" => AppMode::Full,
            "reviews" => AppMode::Reviews,
            _ => return None,
        })
    }

    pub fn id(self) -> &'static str {
        match self {
            AppMode::Full => "full",
            AppMode::Reviews => "reviews",
        }
    }
}

/// Whose review requests the review list shows (`reviewRequestsScope`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewScope {
    /// Requested from you or from a team you're on.
    All,
    /// Requested from you by name.
    Direct,
    /// Requested from one of your teams (not you by name).
    Teams,
}

impl ReviewScope {
    pub const IDS: [&str; 3] = ["all", "direct", "teams"];

    pub fn parse(s: &str) -> Option<ReviewScope> {
        Some(match s {
            "all" => ReviewScope::All,
            "direct" => ReviewScope::Direct,
            "teams" => ReviewScope::Teams,
            _ => return None,
        })
    }

    pub fn id(self) -> &'static str {
        match self {
            ReviewScope::All => "all",
            ReviewScope::Direct => "direct",
            ReviewScope::Teams => "teams",
        }
    }
}

/// How the review page orders its PRs (`reviewSort`). Within a section the
/// most recently updated comes first, except for [`ReviewSort::Oldest`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewSort {
    /// PRs that ask you by name first, then those asking one of your teams.
    Direct,
    /// Grouped by repository (tracked order).
    Repo,
    /// Most recently updated first, one list.
    Updated,
    /// Waiting longest (oldest opened) first, one list.
    Oldest,
}

impl ReviewSort {
    pub const IDS: [&str; 4] = ["direct", "repo", "updated", "oldest"];

    pub fn parse(s: &str) -> Option<ReviewSort> {
        Some(match s {
            "direct" => ReviewSort::Direct,
            "repo" => ReviewSort::Repo,
            "updated" => ReviewSort::Updated,
            "oldest" => ReviewSort::Oldest,
            _ => return None,
        })
    }

    pub fn id(self) -> &'static str {
        match self {
            ReviewSort::Direct => "direct",
            ReviewSort::Repo => "repo",
            ReviewSort::Updated => "updated",
            ReviewSort::Oldest => "oldest",
        }
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

/// `current` with every known key back at its default. Keys the settings
/// model doesn't know (another version's, the TS app's) are kept, and so is
/// `setupCompleted` unless `keep_setup_completed` is false, which makes the
/// next start show the setup wizard.
pub fn reset_to_defaults(current: Settings, keep_setup_completed: bool) -> Settings {
    let known = Settings::default().to_view();
    Settings {
        raw: current
            .raw
            .into_iter()
            .filter(|(k, _)| !known.contains_key(k) && (keep_setup_completed || k != "setupCompleted"))
            .collect(),
        ..Settings::default()
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
    fn reset_keeps_unknown_keys_and_optionally_setup_completed() {
        let s = Settings::parse(r#"{"logLevel":"debug","hideMainBranch":false,"setupCompleted":true,"futureKey":7}"#);
        let kept = reset_to_defaults(s.clone(), true);
        assert_eq!(kept.log_level(), Settings::default().log_level());
        assert!(kept.hide_main_branch);
        assert!(!kept.raw.contains_key("logLevel"));
        assert_eq!(kept.raw.get("futureKey"), Some(&serde_json::json!(7)));
        assert_eq!(kept.raw.get("setupCompleted"), Some(&serde_json::json!(true)));
        let wiped = reset_to_defaults(s, false);
        assert!(!wiped.raw.contains_key("setupCompleted"));
        assert!(wiped.raw.contains_key("futureKey"));
    }

    #[test]
    fn worktree_filters_default_to_on_with_nothing_picked_and_round_trip() {
        let d = Settings::default();
        assert!(d.worktree_filters_enabled && d.worktree_filter_text.is_empty());
        assert_eq!((d.worktree_filter_status.as_str(), d.worktree_filter_pr.as_str(), d.worktree_filter_git.as_str()), ("any", "any", "any"));
        let s = Settings::parse(
            r#"{"worktreeFiltersEnabled":false,"worktreeFilterText":"auth","worktreeFilterStatus":"waiting","worktreeFilterPr":"failing","worktreeFilterGit":"changes"}"#,
        );
        assert!(!s.worktree_filters_enabled);
        assert_eq!(s.worktree_filter_text, "auth");
        let back = Settings::parse(&serde_json::to_string(&s).unwrap());
        assert_eq!((back.worktree_filter_status.as_str(), back.worktree_filter_pr.as_str(), back.worktree_filter_git.as_str()), ("waiting", "failing", "changes"));
    }

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
    fn key_bindings_default_empty_and_round_trip() {
        assert!(Settings::parse("{}").key_bindings.is_empty());
        let s = Settings::parse(r#"{"keyBindings":{"newWorktree":"N"},"hideMainBranch":false}"#);
        assert_eq!(s.key_bindings.get("newWorktree").map(String::as_str), Some("N"));
        assert!(!s.hide_main_branch);
        // A bad value is dropped without resetting the rest.
        let bad = Settings::parse(r#"{"keyBindings":["N"],"hideMainBranch":false}"#);
        assert!(bad.key_bindings.is_empty() && !bad.hide_main_branch);
        assert_eq!(s.to_file_json()["keyBindings"]["newWorktree"], "N");
    }

    #[test]
    fn mouse_is_on_by_default_and_can_be_turned_off() {
        assert!(Settings::parse("{}").mouse_enabled);
        let off = Settings::parse(r#"{"mouseEnabled":false}"#);
        assert!(!off.mouse_enabled);
        assert_eq!(off.to_file_json()["mouseEnabled"], serde_json::Value::Bool(false));
    }

    #[test]
    fn startup_scripts_run_in_the_pane_unless_set_to_window() {
        assert!(!Settings::default().startup_script_in_window());
        assert!(!Settings::parse(r#"{"startupScriptMode":"bogus"}"#).startup_script_in_window());
        assert!(Settings::parse(r#"{"startupScriptMode":"window"}"#).startup_script_in_window());
    }

    #[test]
    fn compact_view_is_off_by_default() {
        assert!(!Settings::parse("{}").compact_view);
        let on = Settings::parse(r#"{"compactView":true}"#);
        assert!(on.compact_view);
        assert_eq!(on.to_file_json()["compactView"], true);
    }

    #[test]
    fn daily_tokens_are_shown_for_all_usage_by_default() {
        let d = Settings::parse("{}");
        assert!(d.show_daily_tokens && !d.daily_tokens_visible_only());
        assert!(!Settings::parse(r#"{"dailyTokensScope":"bogus"}"#).daily_tokens_visible_only());
        assert!(Settings::parse(r#"{"dailyTokensScope":"visible"}"#).daily_tokens_visible_only());
        assert!(!Settings::parse(r#"{"showDailyTokens":false}"#).show_daily_tokens);
        assert_eq!(d.daily_tokens_mode(), DailyTokensMode::Tokens);
        assert_eq!(Settings::parse(r#"{"dailyTokensMode":"bogus"}"#).daily_tokens_mode(), DailyTokensMode::Tokens);
        assert_eq!(Settings::parse(r#"{"dailyTokensMode":"cost"}"#).daily_tokens_mode(), DailyTokensMode::Cost);
    }

    #[test]
    fn review_requests_default_to_me_and_my_teams() {
        let d = Settings::parse("{}");
        assert!(d.review_requests_on() && d.review_hide_drafts && d.review_hide_reviewed);
        assert_eq!(d.app_mode(), AppMode::Full);
        assert!(!d.reviews_mode());
        assert_eq!(Settings::parse(r#"{"appMode":"bogus"}"#).app_mode(), AppMode::Full);
        assert!(Settings::parse(r#"{"appMode":"reviews"}"#).reviews_mode());
        assert_eq!(d.review_scope(), ReviewScope::All);
        assert_eq!(Settings::parse(r#"{"reviewRequestsScope":"bogus"}"#).review_scope(), ReviewScope::All);
        assert_eq!(Settings::parse(r#"{"reviewRequestsScope":"teams"}"#).review_scope(), ReviewScope::Teams);
        assert!(!Settings::parse(r#"{"ghPrStatus":false}"#).review_requests_on());
        assert!(!Settings::parse(r#"{"reviewRequestsEnabled":false}"#).review_requests_on());
        assert_eq!(d.review_sort(), ReviewSort::Direct);
        assert_eq!(Settings::parse(r#"{"reviewSort":"oldest"}"#).review_sort(), ReviewSort::Oldest);
        assert_eq!(Settings::parse(r#"{"reviewSort":"bogus"}"#).review_sort(), ReviewSort::Direct);
        assert!(d.review_groups.is_empty() && d.review_default_group == ReviewGroup::default() && !d.review_badge_total);
        let g = Settings::parse(r#"{"reviewGroups":[{"name":"Mine","asked":"you"},{"authors":["bob"]}],"reviewDefaultGroup":{"badge":false}}"#);
        assert_eq!(g.review_groups.len(), 2);
        assert_eq!((g.review_groups[0].name.as_str(), g.review_groups[0].asked.as_str()), ("Mine", "you"));
        assert_eq!(g.review_groups[1].draft, "any", "missing rules default to any");
        assert!(!g.review_default_group.badge && g.review_default_group.checks == "any");
        // 2.1.x filters become the default group's rules; a new file's is all off.
        let old = Settings::parse(
            r#"{"reviewHideDrafts":true,"reviewHideReviewed":false,"reviewExcludeRepos":"docs, api","reviewSearchQualifiers":" label:x ","reviewSort":"oldest"}"#,
        );
        let dg = &old.review_default_group;
        assert_eq!((dg.draft.as_str(), dg.reviewed.as_str(), dg.sort.as_str()), ("ready", "any", "oldest"));
        assert_eq!((dg.exclude_repos.clone(), dg.query.as_str()), (vec!["docs".to_string(), "api".to_string()], "label:x"));
        assert_eq!(Settings::parse("{}").review_default_group, ReviewGroup::default());
        let set = Settings::parse(r#"{"reviewHideDrafts":true,"reviewDefaultGroup":{}}"#);
        assert_eq!(set.review_default_group.draft, "any", "a saved default group wins");
        // A malformed list is ignored, not the whole file.
        let bad = Settings::parse(r#"{"reviewGroups":"nope","reviewSort":"repo"}"#);
        assert!(bad.review_groups.is_empty() && bad.review_sort() == ReviewSort::Repo);
    }

    #[test]
    fn one_bad_value_does_not_reset_the_rest() {
        let s = Settings::parse(r#"{"pollingIntervalMs":"fast","ide":"zed"}"#);
        assert_eq!(s.polling_interval_ms, 30_000);
        assert_eq!(s.ide, "zed");
    }

    #[test]
    fn opens_in_vscode_by_default() {
        let d = Settings::parse("{}");
        assert_eq!((d.ide.as_str(), d.ide_command.as_str()), ("vscode", ""));
        let c = Settings::parse(r#"{"ide":"custom","ideCommand":"subl -n"}"#);
        assert_eq!((c.ide.as_str(), c.ide_command.as_str()), ("custom", "subl -n"));
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
    fn context_window_defaults_to_1m() {
        assert_eq!(Settings::parse("{}").context_window_tokens, 1_000_000);
        assert_eq!(Settings::parse(r#"{"contextWindowTokens":200000}"#).context_window_tokens, 200_000);
    }

    #[test]
    fn cmux_sync_defaults_on_and_parses() {
        assert!(Settings::parse("{}").cmux_sync);
        assert!(!Settings::parse(r#"{"cmuxSync":false}"#).cmux_sync);
        assert_eq!(Settings::default().to_file_json()["cmuxSync"], true);
    }

    #[test]
    fn check_for_updates_defaults_on_and_parses() {
        assert!(Settings::parse("{}").check_for_updates);
        assert!(!Settings::parse(r#"{"checkForUpdates":false}"#).check_for_updates);
        assert_eq!(Settings::default().to_file_json()["checkForUpdates"], true);
    }

    #[test]
    fn garbage_file_gives_defaults() {
        assert_eq!(Settings::parse("not json").polling_interval_ms, 30_000);
    }
}
