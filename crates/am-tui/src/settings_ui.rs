//! Slim settings editor (`s`). Edits `settings.json` key by key so unknown
//! keys (written by the TS app or a newer version) survive. `e` opens the
//! whole file in the IDE (or $EDITOR) for anything not listed here.

use am_core::context;
use am_core::settings::{Direction, SortCriterion, SortKey, Settings};
use am_core::themes;
use serde_json::{Map, Value};

use crate::app::Key;
use crate::keys_ui::{KeyEditor, KeyOutcome};
use crate::plugins_ui::{self, PluginEditor, PluginOutcome};
use crate::repos_ui::{self, RepoEditor, RepoOutcome};
use crate::review_groups_ui::{Candidates, GroupCounts, GroupsEditor, GroupsOutcome};
use crate::theme_builder::{BuilderOutcome, ThemeBuilder};

#[derive(Debug, Clone, Copy)]
pub enum Kind {
    Bool,
    Choice(&'static [&'static str]),
    Number { step: u64, min: u64 },
    /// A token count: shown as `200k`/`1.0M`, typed as `128k`/`1m`, ←→ step
    /// through `TOKEN_PRESETS`.
    Tokens { min: u64 },
    Text { secret: bool },
    /// Opens the review groups page.
    ReviewGroups,
    Sort,
    /// Built-in and custom themes, listed at runtime.
    Theme,
    /// Opens the theme builder.
    ThemeBuilder,
    /// Opens the harness plugins page.
    Plugins,
    /// Opens the key bindings page.
    KeyBindings,
    /// Opens the repositories page.
    Repos,
    /// Reopens the setup wizard over the current settings.
    RunSetup,
    /// Opens debug.log (or the log folder once it has rotated).
    ViewLogs,
    /// Asks, then puts every setting back to its default and runs setup.
    Reset,
    /// Asks (`y`), then puts the review filters back to their defaults.
    ResetReviewFilters,
    /// Asks (`y`), then removes every review group.
    ResetReviewGroups,
}

pub struct Field {
    pub key: &'static str,
    pub label: &'static str,
    pub help: &'static str,
    pub section: &'static str,
    pub kind: Kind,
}

/// Common context-window sizes ←→ step through on a `Kind::Tokens` field.
const TOKEN_PRESETS: &[u64] = &[32_000, 64_000, 128_000, 200_000, 256_000, 400_000, 500_000, 1_000_000, 2_000_000];

const SOUNDS: &[&str] =
    &["Basso", "Blow", "Bottle", "Frog", "Funk", "Glass", "Hero", "Morse", "Ping", "Pop", "Purr", "Sosumi", "Submarine", "Tink"];

pub const FIELDS: &[Field] = &[
    Field { key: "ide", label: "Open in", help: "Which editor/IDE Enter opens worktrees in; custom runs Editor command", section: "General", kind: Kind::Choice(&am_core::launch::IdeChoice::IDS) },
    Field { key: "ideCommand", label: "Editor command", help: "For Open in = custom, e.g. subl -n; {path} = worktree, else it's appended", section: "General", kind: Kind::Text { secret: false } },
    Field { key: "resumeLastSession", label: "Resume last session on open", help: "Enter also resumes the worktree's most recent agent session", section: "General", kind: Kind::Bool },
    Field { key: "repositories", label: "Repositories…", help: "Add or stop tracking repositories, edit their startup and review scripts", section: "General", kind: Kind::Repos },
    Field { key: "agentPlugins", label: "Harness plugins…", help: "Turn harnesses on/off, install their hooks, pick the default", section: "General", kind: Kind::Plugins },
    Field { key: "keyBindings", label: "Key bindings…", help: "Choose the dashboard's keys (the review page uses them too)", section: "General", kind: Kind::KeyBindings },
    Field { key: "mouseEnabled", label: "Mouse", help: "Click, double-click and scroll; off restores the terminal's own text selection", section: "General", kind: Kind::Bool },
    Field { key: "startupScriptMode", label: "Run startup script in", help: "pane: output in the Jobs pane; window: a new terminal window", section: "General", kind: Kind::Choice(&["pane", "window"]) },
    Field { key: "defaultBranchPrefix", label: "Branch prefix", help: "Prefix for new worktree branches", section: "General", kind: Kind::Text { secret: false } },
    Field { key: "defaultBaseBranch", label: "Base branch", help: "Default base branch for new worktrees", section: "General", kind: Kind::Text { secret: false } },
    Field { key: "pollingIntervalMs", label: "Git refresh (ms)", help: "Full git sweep interval; agent activity refreshes its worktree immediately", section: "General", kind: Kind::Number { step: 5000, min: 5000 } },
    Field { key: "theme", label: "Theme", help: "←→ to cycle; ansi follows your terminal palette", section: "Appearance", kind: Kind::Theme },
    Field { key: "themeBuilder", label: "Theme builder…", help: "Make your own theme from the current one: pick colours, preview live, save", section: "Appearance", kind: Kind::ThemeBuilder },
    Field { key: "compactView", label: "Compact view", help: "Dense layout for small panes: top title lines only, no blank rows, fewer columns, side panes open below", section: "Appearance", kind: Kind::Bool },
    Field { key: "worktreeSort", label: "Sort order", help: "Criteria applied in order; first difference wins", section: "Dashboard", kind: Kind::Sort },
    Field { key: "hideMainBranch", label: "Hide main branch", help: "Hide the main/master checkout from the list", section: "Dashboard", kind: Kind::Bool },
    Field { key: "hideMergedClosedPrs", label: "Hide merged/closed PRs", help: "Unless the agent session is open", section: "Dashboard", kind: Kind::Bool },
    Field { key: "hideIdleDoneAgents", label: "Hide idle/done agents", help: "Only show worktrees with active agents", section: "Dashboard", kind: Kind::Bool },
    Field { key: "hideWithoutLinearTicket", label: "Hide without Linear ticket", help: "Unless the session is open", section: "Dashboard", kind: Kind::Bool },
    Field { key: "showPrStatus", label: "Show PR column", help: "PR number and checks on each row", section: "Dashboard", kind: Kind::Bool },
    Field { key: "showLinearTicket", label: "Show ticket column", help: "Linear identifier on each row", section: "Dashboard", kind: Kind::Bool },
    Field { key: "showGitAheadBehind", label: "Show git column", help: "Ahead/behind/dirty counts", section: "Dashboard", kind: Kind::Bool },
    Field { key: "showLastCommit", label: "Show commit age", help: "Age of the last commit", section: "Dashboard", kind: Kind::Bool },
    Field { key: "showContextWindow", label: "Show context window", help: "Bar of the agent's context used, coloured as it nears the limit", section: "Dashboard", kind: Kind::Bool },
    Field { key: "showDailyTokens", label: "Show today's token usage", help: "Tokens used today (input, cache and output) at the bottom right of the list", section: "Dashboard", kind: Kind::Bool },
    Field { key: "dailyTokensScope", label: "Token usage counts", help: "all: every session today; visible: only the rows shown in the list", section: "Dashboard", kind: Kind::Choice(&["all", "visible"]) },
    Field { key: "dailyTokensMode", label: "Token usage shows", help: "tokens: all billed; weighted: by price (cache read 0.1×); cost: est. $; breakdown: per class", section: "Dashboard", kind: Kind::Choice(&am_core::settings::DailyTokensMode::IDS) },
    Field { key: "contextWindowTokens", label: "Context window (tokens)", help: "Bar scale; a session past it doubles the scale until it fits (⏎ to type, e.g. 128k)", section: "Dashboard", kind: Kind::Tokens { min: context::MIN_WINDOW } },
    Field { key: "showRunningProcesses", label: "Detect dev servers", help: "Show processes running inside each worktree", section: "Dashboard", kind: Kind::Bool },
    Field { key: "runningProcessFilter", label: "Process filter", help: "Only show processes and agent commands whose command line contains this", section: "Dashboard", kind: Kind::Text { secret: false } },
    Field { key: "audioNotifications", label: "Sounds", help: "Play a sound when an agent is waiting or done", section: "Notifications", kind: Kind::Bool },
    Field { key: "checkForUpdates", label: "Check for updates", help: "Show a notice when a newer am release is on GitHub", section: "Notifications", kind: Kind::Bool },
    Field { key: "audioWaitingSound", label: "Waiting sound", help: "Played when an agent needs you", section: "Notifications", kind: Kind::Choice(SOUNDS) },
    Field { key: "audioDoneSound", label: "Done sound", help: "Played when an agent finishes", section: "Notifications", kind: Kind::Choice(SOUNDS) },
    Field { key: "ghPrStatus", label: "Fetch PRs", help: "GitHub PR and CI status via gh", section: "GitHub", kind: Kind::Bool },
    Field { key: "ghPollingIntervalMs", label: "PR refresh (ms)", help: "How often to fetch PR status", section: "GitHub", kind: Kind::Number { step: 30000, min: 30000 } },
    Field { key: "ghRefreshOnManual", label: "Refresh on r", help: "Include GitHub when refreshing manually", section: "GitHub", kind: Kind::Bool },
    Field { key: "reviewRequestsEnabled", label: "Show review requests", help: "PRs waiting on your review: the v page and the N to review badge", section: "Review requests", kind: Kind::Bool },
    Field { key: "reviewRequestsScope", label: "Requested from", help: "all: you or a team you're on; direct: you by name; teams: only your teams", section: "Review requests", kind: Kind::Choice(&am_core::settings::ReviewScope::IDS) },
    Field { key: "reviewGroups", label: "Groups…", help: "Each group takes the PRs it matches (with its own order and filters), the rest fall to the next", section: "Review requests", kind: Kind::ReviewGroups },
    Field { key: "reviewBadgeTotal", label: "Badge: total on GitHub", help: "Also count every open PR asking you or your teams on GitHub, ignoring filters", section: "Review requests", kind: Kind::Bool },
    Field { key: "resetReviewFilters", label: "Reset filters…", help: "Requested from: you + your teams; the default group's filters off; no GitHub total (groups kept)", section: "Review requests", kind: Kind::ResetReviewFilters },
    Field { key: "resetReviewGroups", label: "Reset groups…", help: "Remove every review group (the default group and filters kept)", section: "Review requests", kind: Kind::ResetReviewGroups },
    Field { key: "reviewPollingIntervalMs", label: "Review refresh (ms)", help: "How often to look for new review requests", section: "Review requests", kind: Kind::Number { step: 30000, min: 30000 } },
    Field { key: "linearEnabled", label: "Fetch tickets", help: "Linked Linear tickets for each branch", section: "Linear", kind: Kind::Bool },
    Field { key: "linearApiKey", label: "API key", help: "Personal API key from Linear settings", section: "Linear", kind: Kind::Text { secret: true } },
    Field { key: "linearPollingIntervalMs", label: "Ticket refresh (ms)", help: "How often to fetch Linear tickets", section: "Linear", kind: Kind::Number { step: 30000, min: 30000 } },
    Field { key: "linearRefreshOnManual", label: "Refresh on r", help: "Include Linear when refreshing manually", section: "Linear", kind: Kind::Bool },
    Field { key: "linearUseDesktopApp", label: "Open in desktop app", help: "linear:// links instead of the browser", section: "Linear", kind: Kind::Bool },
    Field { key: "linearAutoNickname", label: "Nickname from ticket", help: "Name worktrees after their Linear ticket title", section: "Linear", kind: Kind::Bool },
    Field { key: "worktreeLimitEnabled", label: "Limit worktrees", help: "Block creating more than N worktrees per repo", section: "Limits", kind: Kind::Bool },
    Field { key: "maxWorktrees", label: "Max per repo", help: "Excludes the main checkout", section: "Limits", kind: Kind::Number { step: 1, min: 1 } },
    Field { key: "logLevel", label: "Log level", help: "debug.log verbosity", section: "Logging", kind: Kind::Choice(&["debug", "info", "warn", "error"]) },
    Field { key: "maxLogSizeMb", label: "Rotate log at (MB)", help: "debug.log is renamed to debug.log.1 past this size", section: "Logging", kind: Kind::Number { step: 1, min: 1 } },
    // An action, not a setting: its key is never written.
    Field { key: "viewLogs", label: "View logs…", help: "Open debug.log in the editor (the log folder once it has rotated)", section: "Logging", kind: Kind::ViewLogs },
    // Actions, not settings: their keys are never written.
    Field { key: "rerunSetup", label: "Run setup again…", help: "Walk the setup wizard again; nothing is saved until you finish (esc cancels)", section: "Setup", kind: Kind::RunSetup },
    Field { key: "resetSettings", label: "Reset settings…", help: "Defaults for every setting, then setup (repos, themes, scripts, hooks kept)", section: "Setup", kind: Kind::Reset },
];

/// The rows of the review page's filter box (`f`), a cut-down editor.
pub const REVIEW_FILTERS: &[&str] =
    &["reviewRequestsScope", "reviewGroups", "reviewBadgeTotal", "resetReviewFilters", "resetReviewGroups"];

pub enum Outcome {
    None,
    Close,
    Changed(Box<Settings>),
    EditFile,
    /// Open the folder browser to add a repository.
    AddRepo,
    /// Open this repo's startup or review script (created from the template if missing).
    EditScript(String, am_core::worktrees::ScriptKind),
    /// A repository was removed: the dashboard should refresh.
    ReposChanged,
    /// Run setup again: close the editor and open the wizard.
    RunSetup,
    /// View logs: open debug.log (or its folder).
    ViewLogs,
    /// Ask before resetting every setting.
    ConfirmReset,
    /// The groups page opened: look up who can author PRs on GitHub.
    LoadPeople,
}

pub struct SettingsEditor {
    pub cursor: usize,
    pub editing: Option<String>,
    pub sort: Option<SortEditor>,
    pub builder: Option<ThemeBuilder>,
    pub plugins: Option<PluginEditor>,
    pub keys: Option<KeyEditor>,
    pub repos: Option<RepoEditor>,
    /// Tracked repositories (the app keeps it in step with its snapshot).
    pub repo_count: usize,
    pub values: Map<String, Value>,
    pub error: Option<String>,
    /// A review reset waiting for `y` (the field's key).
    pub confirm: Option<&'static str>,
    /// Only these keys are shown and reachable (the review filter box).
    pub only: Option<&'static [&'static str]>,
    /// Review filters → Groups….
    pub groups: Option<GroupsEditor>,
    /// Autocomplete for the groups page (the app keeps it current).
    pub candidates: Candidates,
    /// PRs per review group under the saved groups (the app keeps it current).
    pub group_counts: GroupCounts,
}

impl SettingsEditor {
    pub fn new(settings: &Settings) -> SettingsEditor {
        let values = match serde_json::to_value(settings) {
            Ok(Value::Object(m)) => m,
            _ => Map::new(),
        };
        SettingsEditor { cursor: 0, editing: None, sort: None, builder: None, plugins: None, keys: None, repos: None, repo_count: 0, values, error: None, confirm: None, only: None, groups: None, candidates: Candidates::default(), group_counts: GroupCounts::default() }
    }

    /// An editor showing only the `keys` fields, starting on the first.
    pub fn only(settings: &Settings, keys: &'static [&'static str]) -> SettingsEditor {
        let mut e = SettingsEditor::new(settings);
        e.only = Some(keys);
        e.cursor = e.fields().first().copied().unwrap_or(0);
        e
    }

    /// Indices into `FIELDS` this editor shows.
    pub fn fields(&self) -> Vec<usize> {
        (0..FIELDS.len()).filter(|&i| self.only.is_none_or(|keys| keys.contains(&FIELDS[i].key))).collect()
    }

    fn move_cursor(&mut self, dir: isize) {
        let fields = self.fields();
        let pos = fields.iter().position(|&i| i == self.cursor).unwrap_or(0) as isize;
        self.cursor = fields[(pos + dir).clamp(0, fields.len() as isize - 1) as usize];
    }

    /// A click on field `i`: selects it, or changes it (like ⏎) when it is
    /// already selected. Ignored while a sub-page (harness plugins, key
    /// bindings, sort editor, theme builder) covers the field list.
    pub fn click(&mut self, i: usize) -> Outcome {
        if self.plugins.is_some()
            || self.groups.is_some()
            || self.keys.is_some()
            || self.repos.is_some()
            || self.sort.is_some()
            || self.builder.is_some()
            || !self.fields().contains(&i)
        {
            return Outcome::None;
        }
        if i == self.cursor {
            if self.editing.is_some() {
                return Outcome::None;
            }
            return self.on_key(Key::Enter);
        }
        self.cursor = i;
        self.editing = None;
        self.error = None;
        self.confirm = None;
        Outcome::None
    }

    pub fn field(&self) -> &'static Field {
        &FIELDS[self.cursor]
    }

    pub fn display(&self, f: &Field) -> String {
        let v = self.values.get(f.key);
        match f.kind {
            Kind::Bool => if v.and_then(Value::as_bool).unwrap_or(false) { "on" } else { "off" }.into(),
            Kind::Text { secret: true } => match v.and_then(Value::as_str) {
                Some(s) if !s.is_empty() => format!("••••{}", &s[s.len().saturating_sub(4)..]),
                _ => "not set".into(),
            },
            Kind::ThemeBuilder | Kind::RunSetup | Kind::ViewLogs => "⏎ open".into(),
            Kind::Reset | Kind::ResetReviewFilters => "⏎ reset".into(),
            Kind::ResetReviewGroups => match self.settings().review_groups.len() {
                0 => "no groups".into(),
                n => format!("⏎ remove {n}"),
            },
            Kind::Repos => match self.repo_count {
                1 => "1 tracked".into(),
                n => format!("{n} tracked"),
            },
            Kind::KeyBindings => match self.settings().key_bindings.len() {
                0 => "defaults".into(),
                n => format!("{n} changed"),
            },
            Kind::Tokens { .. } => v.and_then(Value::as_u64).map(context::short_tokens).unwrap_or_else(|| "—".into()),
            Kind::ReviewGroups => {
                let s = self.settings();
                let groups = match s.review_groups.len() {
                    0 => "no groups".to_string(),
                    1 => "1 group".into(),
                    n => format!("{n} groups"),
                };
                match s.review_default_group.summary() {
                    r if r.is_empty() => groups,
                    r => format!("{groups} · rest: {r}"),
                }
            }
            Kind::Plugins => {
                let s = self.settings();
                // A count: there are too many built-ins to name; the page lists them.
                let on = am_core::harness::enabled(&s).len();
                format!("{on} of {} on · default {}", am_core::harness::all().len(), s.default_agent().id())
            }
            Kind::Sort => {
                let crits: Vec<SortCriterion> = v.cloned().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
                let on: Vec<&str> = crits.iter().filter(|c| c.enabled).map(|c| sort_label(c.key)).collect();
                if on.is_empty() { "none".into() } else { on.join(" › ") }
            }
            _ => match v {
                Some(Value::String(s)) if s.is_empty() => "—".into(),
                Some(Value::String(s)) => s.clone(),
                Some(other) => other.to_string(),
                None => "—".into(),
            },
        }
    }

    /// The editor's current values as typed settings.
    fn settings(&self) -> Settings {
        serde_json::from_value(Value::Object(self.values.clone())).unwrap_or_default()
    }

    pub fn on_key(&mut self, key: Key) -> Outcome {
        if let Some(repos) = &mut self.repos {
            let out = repos.on_key(key);
            return self.repo_effect(out);
        }
        if let Some(plugins) = &mut self.plugins {
            let out = plugins.on_key(key);
            return self.plugin_effect(out);
        }
        if let Some(ed) = &mut self.groups {
            return match ed.on_key(key, &self.candidates) {
                GroupsOutcome::Stay => Outcome::None,
                GroupsOutcome::Close => {
                    self.groups = None;
                    Outcome::None
                }
                GroupsOutcome::Save => self.save_groups(),
            };
        }
        if let Some(keys) = &mut self.keys {
            return match keys.on_key(key) {
                KeyOutcome::Stay => Outcome::None,
                KeyOutcome::Close => {
                    self.keys = None;
                    Outcome::None
                }
                KeyOutcome::Save(bindings) => self.set("keyBindings", serde_json::to_value(bindings).unwrap_or_default()),
            };
        }
        if let Some(builder) = &mut self.builder {
            match builder.on_key(key) {
                BuilderOutcome::Stay => {}
                BuilderOutcome::Close => self.builder = None,
                BuilderOutcome::Save(spec) => match themes::save_custom(&spec) {
                    Ok(path) => {
                        am_core::info!("theme", "Saved theme {} (base {:?}) to {}", spec.name, spec.base, path.display());
                        self.builder = None;
                        return self.set("theme", Value::from(spec.name));
                    }
                    Err(e) => builder.error = Some(e),
                },
            }
            return Outcome::None;
        }
        if let Some(sort) = &mut self.sort {
            match sort.on_key(key) {
                SortOutcome::Stay => return Outcome::None,
                SortOutcome::Done(Some(c)) => {
                    self.sort = None;
                    return self.set("worktreeSort", serde_json::to_value(c).unwrap_or_default());
                }
                SortOutcome::Done(None) => {
                    self.sort = None;
                    return Outcome::None;
                }
            }
        }
        if let Some(buf) = &mut self.editing {
            match key {
                Key::Esc => self.editing = None,
                Key::Enter => {
                    let text = std::mem::take(buf);
                    self.editing = None;
                    let f = self.field();
                    let value = match f.kind {
                        Kind::Number { min, .. } => match text.trim().parse::<u64>() {
                            Ok(n) if n >= min => Value::from(n),
                            _ => {
                                self.error = Some(format!("Enter a whole number ≥ {min}"));
                                return Outcome::None;
                            }
                        },
                        Kind::Tokens { min } => match context::parse_tokens(&text) {
                            Some(n) if n >= min => Value::from(n),
                            _ => {
                                self.error = Some(format!("Enter a token count ≥ {} (e.g. 128k, 200k, 1m)", context::short_tokens(min)));
                                return Outcome::None;
                            }
                        },
                        _ => Value::from(text),
                    };
                    return self.set(f.key, value);
                }
                Key::Backspace => {
                    buf.pop();
                }
                Key::Char(c) => buf.push(c),
                _ => {}
            }
            return Outcome::None;
        }
        if let Some(which) = self.confirm.take() {
            return if key == Key::Char('y') { self.reset_reviews(which) } else { Outcome::None };
        }
        self.error = None;
        match key {
            Key::Esc | Key::Char('q') | Key::Char('s') => return Outcome::Close,
            Key::Char('f') if self.only.is_some() => return Outcome::Close,
            Key::Char('e') if self.only.is_none() => return Outcome::EditFile,
            Key::Down | Key::Char('j') => self.move_cursor(1),
            Key::Up | Key::Char('k') => self.move_cursor(-1),
            Key::Enter | Key::Char(' ') => return self.activate(),
            Key::Right | Key::Char('l') => return self.step(1),
            Key::Left | Key::Char('h') => return self.step(-1),
            _ => {}
        }
        Outcome::None
    }

    fn activate(&mut self) -> Outcome {
        let f = self.field();
        match f.kind {
            Kind::Bool => {
                let cur = self.values.get(f.key).and_then(Value::as_bool).unwrap_or(false);
                self.set(f.key, Value::Bool(!cur))
            }
            Kind::Choice(_) | Kind::Theme => self.step(1),
            Kind::ThemeBuilder => {
                self.builder = Some(open_builder(self.values.get("theme").and_then(Value::as_str).unwrap_or("ansi")));
                Outcome::None
            }
            Kind::Plugins => {
                let s = self.settings();
                let errors = am_core::harness::plugin_errors().to_vec();
                let dir = am_core::hooks_installer::tilde(&am_core::harness::plugins_dir());
                self.plugins = Some(PluginEditor::new(plugins_ui::load_rows(&s), &s.default_agent, errors, dir));
                Outcome::None
            }
            Kind::Repos => {
                self.repos = Some(RepoEditor::new(repos_ui::load_rows()));
                Outcome::None
            }
            Kind::KeyBindings => {
                self.keys = Some(KeyEditor::new(crate::keymap::Keymap::from_settings(&self.settings())));
                Outcome::None
            }
            Kind::Number { .. } => {
                self.editing = Some(self.values.get(f.key).map(|v| v.to_string()).unwrap_or_default());
                Outcome::None
            }
            Kind::Tokens { .. } => {
                self.editing = Some(self.values.get(f.key).and_then(Value::as_u64).map(context::short_tokens).unwrap_or_default());
                Outcome::None
            }
            Kind::Text { secret } => {
                let cur = self.values.get(f.key).and_then(Value::as_str).unwrap_or("");
                self.editing = Some(if secret { String::new() } else { cur.to_string() });
                Outcome::None
            }
            Kind::ReviewGroups => {
                let s = self.settings();
                self.groups = Some(GroupsEditor::new(s.review_groups, s.review_default_group));
                Outcome::LoadPeople
            }
            Kind::RunSetup => Outcome::RunSetup,
            Kind::ViewLogs => Outcome::ViewLogs,
            Kind::Reset => Outcome::ConfirmReset,
            Kind::ResetReviewFilters | Kind::ResetReviewGroups => {
                self.confirm = Some(f.key);
                Outcome::None
            }
            Kind::Sort => {
                let crits = self.values.get(f.key).cloned().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
                self.sort = Some(SortEditor::new(crits));
                Outcome::None
            }
        }
    }

    fn step(&mut self, dir: i64) -> Outcome {
        let f = self.field();
        match f.kind {
            Kind::Choice(opts) => {
                let cur = self.values.get(f.key).and_then(Value::as_str).unwrap_or("");
                let i = opts.iter().position(|o| *o == cur).unwrap_or(0) as i64;
                let next = (i + dir).rem_euclid(opts.len() as i64) as usize;
                self.set(f.key, Value::from(opts[next]))
            }
            Kind::Theme => {
                let names = themes::all_names();
                let cur = self.values.get(f.key).and_then(Value::as_str).unwrap_or("");
                let cur = themes::builtin(cur).map(|t| t.name).unwrap_or_else(|| cur.to_string());
                let i = names.iter().position(|n| *n == cur).map(|i| i as i64).unwrap_or(if dir > 0 { -1 } else { 0 });
                let next = (i + dir).rem_euclid(names.len() as i64) as usize;
                self.set(f.key, Value::from(names[next].clone()))
            }
            Kind::Number { step, min } => {
                let cur = self.values.get(f.key).and_then(Value::as_u64).unwrap_or(min);
                let next = if dir > 0 { cur + step } else { cur.saturating_sub(step).max(min) };
                self.set(f.key, Value::from(next))
            }
            Kind::Tokens { min } => {
                let cur = self.values.get(f.key).and_then(Value::as_u64).unwrap_or(min);
                let next = if dir > 0 {
                    TOKEN_PRESETS.iter().copied().find(|&p| p > cur)
                } else {
                    TOKEN_PRESETS.iter().copied().rev().find(|&p| p < cur && p >= min)
                };
                match next {
                    Some(n) => self.set(f.key, Value::from(n)),
                    None => Outcome::None,
                }
            }
            Kind::Bool => self.activate(),
            _ => Outcome::None,
        }
    }

    fn set(&mut self, key: &str, value: Value) -> Outcome {
        self.values.insert(key.to_string(), value.clone());
        match persist(key, value) {
            Ok(s) => Outcome::Changed(Box::new(s)),
            Err(e) => {
                self.error = Some(e);
                Outcome::None
            }
        }
    }
}

impl SettingsEditor {
    /// Run a confirmed review reset: `resetReviewFilters` (scope, the
    /// default group, the GitHub total back to defaults; groups kept) or
    /// `resetReviewGroups` (every group removed).
    fn reset_reviews(&mut self, which: &str) -> Outcome {
        let d = Settings::default();
        let keys: Vec<(&str, Value)> = match which {
            "resetReviewFilters" => vec![
                ("reviewRequestsScope", Value::from(d.review_requests_scope)),
                ("reviewBadgeTotal", Value::Bool(d.review_badge_total)),
                ("reviewDefaultGroup", serde_json::to_value(&d.review_default_group).unwrap_or_default()),
            ],
            _ => vec![("reviewGroups", Value::Array(Vec::new()))],
        };
        am_core::info!("settings", "Review reset via TUI: {which}");
        let mut out = Outcome::None;
        for (key, value) in keys {
            out = self.set(key, value);
            if !matches!(out, Outcome::Changed(_)) {
                break;
            }
        }
        out
    }

    /// Persist the groups page's groups and default group.
    fn save_groups(&mut self) -> Outcome {
        let Some(ed) = &self.groups else { return Outcome::None };
        let (groups, default) = (serde_json::to_value(&ed.groups), serde_json::to_value(&ed.default));
        let (Ok(groups), Ok(default)) = (groups, default) else { return Outcome::None };
        match self.set("reviewGroups", groups) {
            Outcome::Changed(_) => self.set("reviewDefaultGroup", default),
            other => other,
        }
    }

    /// Apply a repositories-page action. Removing runs here; adding and
    /// editing a script need the app (folder browser, editor launch).
    fn repo_effect(&mut self, out: RepoOutcome) -> Outcome {
        let name = |ed: &RepoEditor, id: &str| ed.rows.iter().find(|r| r.id == id).map(|r| r.name.clone()).unwrap_or_default();
        let (message, outcome) = match out {
            RepoOutcome::Stay => return Outcome::None,
            RepoOutcome::Close => {
                self.repos = None;
                return Outcome::None;
            }
            RepoOutcome::Add => return Outcome::AddRepo,
            RepoOutcome::EditScript(id, kind) => return Outcome::EditScript(id, kind),
            RepoOutcome::Remove(id) => {
                let name = self.repos.as_ref().map(|ed| name(ed, &id)).unwrap_or_default();
                let result = am_core::db::open().map_err(|e| e.to_string()).and_then(|c| am_core::worktrees::remove_repository(&c, &id));
                match result {
                    Ok(()) => ((true, format!("Stopped tracking {name} · nothing on disk was touched")), Outcome::ReposChanged),
                    Err(e) => ((false, format!("Couldn't remove {name}: {e}")), Outcome::None),
                }
            }
            RepoOutcome::RemoveScript(id, kind) => {
                let name = self.repos.as_ref().map(|ed| name(ed, &id)).unwrap_or_default();
                match kind.remove(&id) {
                    Ok(_) => {
                        am_core::info!("scripts", "Removed {} for {name} via settings", kind.label());
                        ((true, format!("Removed the {} for {name}", kind.label())), Outcome::None)
                    }
                    Err(e) => ((false, format!("Couldn't remove the script: {e}")), Outcome::None),
                }
            }
        };
        self.refresh_repos(Some(message));
        outcome
    }

    /// Reload the repositories page (after an add, remove or script edit).
    pub fn refresh_repos(&mut self, message: Option<(bool, String)>) {
        if let Some(ed) = &mut self.repos {
            ed.refresh(repos_ui::load_rows(), message);
            self.repo_count = ed.rows.len();
        }
    }

    /// Apply an agent-plugins action: persist `disabledAgents` /
    /// `defaultAgent`, install or remove the plugin's hooks, refresh the rows.
    fn plugin_effect(&mut self, out: PluginOutcome) -> Outcome {
        use am_core::harness::HarnessKind;
        use am_core::hooks_installer as hi;
        let (id, mut outcome) = match &out {
            PluginOutcome::Stay => return Outcome::None,
            PluginOutcome::Close => {
                self.plugins = None;
                return Outcome::None;
            }
            PluginOutcome::SetEnabled(id, _) | PluginOutcome::InstallHooks(id) | PluginOutcome::SetDefault(id) => (id.clone(), Outcome::None),
        };
        let Some(kind) = HarnessKind::get(&id) else { return Outcome::None };
        let p = kind.provider();
        let message = match out {
            PluginOutcome::SetEnabled(_, on) => {
                let mut disabled: Vec<String> = self.settings().disabled_agents;
                disabled.retain(|d| *d != id);
                if !on {
                    disabled.push(id.clone());
                }
                outcome = self.set("disabledAgents", Value::from(disabled));
                am_core::info!("agents", "Plugin {id} turned {} via settings", if on { "on" } else { "off" });
                let s = self.settings();
                // The default must stay usable: hand it to the first enabled agent.
                if !on && s.default_agent().id() == id
                    && let Some(next) = am_core::harness::enabled(&s).first()
                {
                    outcome = self.set("defaultAgent", Value::from(next.id()));
                }
                match (on, hi::is_hooks_installed(kind)) {
                    (true, _) if kind == HarnessKind::CLAUDE || p.is_present() => match hi::install_hooks(kind) {
                        Ok(_) => (true, format!("{} on · hooks installed into {}", p.name(), hi::display_path(p))),
                        Err(e) => (false, format!("{} on, but hooks failed: {e}", p.name())),
                    },
                    (true, _) => (true, format!("{} on · not set up on this machine yet (i installs hooks anyway)", p.name())),
                    (false, true) => match hi::uninstall_hooks(kind) {
                        Ok(()) => (true, format!("{} off · hooks removed from {}", p.name(), hi::display_path(p))),
                        Err(e) => (false, format!("{} off, but removing hooks failed: {e}", p.name())),
                    },
                    (false, false) => (true, format!("{} off", p.name())),
                }
            }
            PluginOutcome::InstallHooks(_) => match hi::install_hooks(kind) {
                Ok(_) => (true, format!("Hooks installed into {}", hi::display_path(p))),
                Err(e) => (false, format!("Couldn't install hooks: {e}")),
            },
            PluginOutcome::SetDefault(_) => {
                outcome = self.set("defaultAgent", Value::from(id.as_str()));
                (true, format!("Enter now starts {} where no session has run", p.name()))
            }
            PluginOutcome::Stay | PluginOutcome::Close => unreachable!(),
        };
        let s = self.settings();
        if let Some(ed) = &mut self.plugins {
            ed.refresh(plugins_ui::load_rows(&s), &s.default_agent, message);
        }
        outcome
    }
}

/// Seed the builder: a custom theme is edited in place, a built-in is copied.
/// Every theme is resolved here so the builder itself never reads files.
fn open_builder(current: &str) -> ThemeBuilder {
    let stored = (!themes::is_builtin(current))
        .then(|| themes::read_custom_in(&am_core::paths::themes_dir(), current).ok())
        .flatten();
    let catalog = themes::all_names().iter().filter_map(|n| themes::load(n).ok()).collect();
    am_core::debug!("theme", "Opening builder on {current} (custom: {})", stored.is_some());
    ThemeBuilder::open(current, stored, catalog)
}

/// Write one key into settings.json, keeping every other key (known or not)
/// exactly as it was, then re-read the result. Same format as the TS
/// `saveSettings` (2-space JSON + newline).
pub(crate) fn persist(key: &str, value: Value) -> Result<Settings, String> {
    let path = am_core::paths::settings_path();
    let mut map = match std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok()) {
        Some(Value::Object(m)) => m,
        _ => Map::new(),
    };
    map.insert(key.to_string(), value);
    let text = serde_json::to_string_pretty(&Value::Object(map)).map_err(|e| e.to_string())? + "\n";
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // Write-then-rename so a crash can't leave a half-written settings file.
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &text).and_then(|_| std::fs::rename(&tmp, &path)).map_err(|e| format!("Couldn't save settings: {e}"))?;
    am_core::info!("settings", "Set {key} via TUI");
    Ok(Settings::parse(&text))
}

pub fn sort_label(k: SortKey) -> &'static str {
    match k {
        SortKey::IsMain => "main last",
        SortKey::Repo => "repo",
        SortKey::LinearTicket => "ticket",
        SortKey::LinearProject => "project",
        SortKey::AgentStatus => "agent status",
        SortKey::LastActivity => "last activity",
        SortKey::CreatedAt => "created",
        SortKey::BranchName => "branch",
        SortKey::PrStatus => "PR status",
        SortKey::GitDirty => "uncommitted",
    }
}

pub enum SortOutcome {
    Stay,
    Done(Option<Vec<SortCriterion>>),
}

/// Sort-order sub-editor: space toggles, d flips direction, J/K move.
pub struct SortEditor {
    pub items: Vec<SortCriterion>,
    pub cursor: usize,
}

impl SortEditor {
    fn new(items: Vec<SortCriterion>) -> SortEditor {
        SortEditor { items, cursor: 0 }
    }

    fn on_key(&mut self, key: Key) -> SortOutcome {
        let n = self.items.len();
        if n == 0 {
            return SortOutcome::Done(None);
        }
        match key {
            Key::Esc => return SortOutcome::Done(None),
            Key::Enter | Key::Char('q') => return SortOutcome::Done(Some(self.items.clone())),
            Key::Down | Key::Char('j') => self.cursor = (self.cursor + 1).min(n - 1),
            Key::Up | Key::Char('k') => self.cursor = self.cursor.saturating_sub(1),
            Key::Char(' ') => self.items[self.cursor].enabled ^= true,
            Key::Char('d') => {
                let c = &mut self.items[self.cursor];
                c.direction = match c.direction {
                    Direction::Asc => Direction::Desc,
                    Direction::Desc => Direction::Asc,
                };
            }
            Key::Char('J') if self.cursor + 1 < n => {
                self.items.swap(self.cursor, self.cursor + 1);
                self.cursor += 1;
            }
            Key::Char('K') if self.cursor > 0 => {
                self.items.swap(self.cursor, self.cursor - 1);
                self.cursor -= 1;
            }
            _ => {}
        }
        SortOutcome::Stay
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// All editor tests share one temp AM_DATA_DIR (env is process-global),
    /// so they hold a lock while they touch settings.json (other modules'
    /// tests that touch the data dir take it too).
    pub(crate) fn data_dir() -> (std::sync::MutexGuard<'static, ()>, &'static std::path::Path) {
        static DIR: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let d = DIR.get_or_init(|| tempfile::tempdir().unwrap());
        // SAFETY: only these tests read AM_DATA_DIR, and they hold LOCK.
        unsafe { std::env::set_var("AM_DATA_DIR", d.path()) };
        let _ = std::fs::remove_file(d.path().join("settings.json"));
        (guard, d.path())
    }

    #[test]
    fn repositories_page_removes_scripts_and_repos() {
        use am_core::worktrees::ScriptKind;
        let (_g, dir) = data_dir();
        let conn = am_core::db::open().unwrap();
        let repo = am_core::db::add_repository(&conn, &dir.join("proj").display().to_string(), "proj").unwrap();
        am_core::worktrees::create_script(&repo.id).unwrap();
        let mut ed = SettingsEditor::new(&Settings::default());
        ed.cursor = FIELDS.iter().position(|f| f.key == "repositories").unwrap();
        ed.on_key(Key::Enter);
        let rows = &ed.repos.as_ref().unwrap().rows;
        assert_eq!(rows.len(), 1);
        assert!(rows[0].script && !rows[0].exists);
        assert_eq!(ed.repo_count, 0, "set by the app, not by opening the page");

        assert!(matches!(ed.on_key(Key::Char('a')), Outcome::AddRepo));
        assert!(matches!(ed.on_key(Key::Char('s')), Outcome::EditScript(id, ScriptKind::Startup) if id == repo.id));
        assert!(matches!(ed.on_key(Key::Char('S')), Outcome::None));
        assert!(!am_core::worktrees::has_script(&repo.id));
        assert!(!ed.repos.as_ref().unwrap().rows[0].script);

        // The review script sits beside it, with its own keys.
        assert!(matches!(ed.on_key(Key::Char('r')), Outcome::EditScript(id, ScriptKind::Review) if id == repo.id));
        ScriptKind::Review.create(&repo.id).unwrap();
        ed.refresh_repos(None);
        assert!(ed.repos.as_ref().unwrap().rows[0].review_script);
        assert!(matches!(ed.on_key(Key::Char('R')), Outcome::None));
        assert!(!ScriptKind::Review.exists(&repo.id));
        assert!(ed.repos.as_ref().unwrap().message.as_ref().is_some_and(|(ok, m)| *ok && m.contains("review script")));

        ed.on_key(Key::Char('x'));
        assert!(matches!(ed.on_key(Key::Char('y')), Outcome::ReposChanged));
        assert!(am_core::db::repository_by_id(&conn, &repo.id).unwrap().is_none());
        let page = ed.repos.as_ref().unwrap();
        assert!(page.rows.is_empty());
        assert!(page.message.as_ref().is_some_and(|(ok, m)| *ok && m.contains("Stopped tracking proj")));
        assert_eq!(ed.repo_count, 0);

        assert!(matches!(ed.on_key(Key::Esc), Outcome::None));
        assert!(ed.repos.is_none());
    }

    fn goto(ed: &mut SettingsEditor, key: &str) {
        ed.cursor = FIELDS.iter().position(|f| f.key == key).unwrap();
    }

    #[test]
    fn setup_rows_are_actions_and_never_written() {
        let (_lock, dir) = data_dir();
        let mut ed = SettingsEditor::new(&Settings::default());
        goto(&mut ed, "rerunSetup");
        assert_eq!(ed.display(ed.field()), "⏎ open");
        assert!(matches!(ed.on_key(Key::Enter), Outcome::RunSetup));
        assert!(matches!(ed.on_key(Key::Right), Outcome::None));
        goto(&mut ed, "viewLogs");
        assert_eq!(ed.display(ed.field()), "⏎ open");
        assert!(matches!(ed.on_key(Key::Enter), Outcome::ViewLogs));
        goto(&mut ed, "resetSettings");
        assert!(matches!(ed.on_key(Key::Enter), Outcome::ConfirmReset));
        assert!(!dir.join("settings.json").exists());
    }

    #[test]
    fn click_selects_a_field_then_changes_it() {
        let (_lock, _) = data_dir();
        let mut ed = SettingsEditor::new(&Settings::default());
        let i = FIELDS.iter().position(|f| f.key == "mouseEnabled").unwrap();
        assert!(matches!(ed.click(i), Outcome::None));
        assert_eq!(ed.cursor, i);
        let Outcome::Changed(s) = ed.click(i) else { panic!("expected change") };
        assert!(!s.mouse_enabled);
    }

    #[test]
    fn edits_preserve_unknown_keys_and_apply() {
        let (_lock, dir) = data_dir();
        std::fs::write(dir.join("settings.json"), r#"{"showIdeStatus":true,"hideMainBranch":true}"#).unwrap();
        let mut ed = SettingsEditor::new(&Settings::parse(r#"{"hideMainBranch":true}"#));

        goto(&mut ed, "hideMainBranch");
        let Outcome::Changed(s) = ed.on_key(Key::Char(' ')) else { panic!("expected change") };
        assert!(!s.hide_main_branch);

        goto(&mut ed, "theme");
        let Outcome::Changed(s) = ed.on_key(Key::Right) else { panic!() };
        assert_eq!(s.theme, "catppuccin-mocha");

        goto(&mut ed, "maxWorktrees");
        ed.on_key(Key::Enter);
        ed.on_key(Key::Backspace);
        ed.on_key(Key::Char('8'));
        let Outcome::Changed(s) = ed.on_key(Key::Enter) else { panic!() };
        assert_eq!(s.max_worktrees, 8);

        let saved: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap()).unwrap();
        assert_eq!(saved["showIdeStatus"], true, "unknown keys survive");
        assert_eq!(saved["hideMainBranch"], false);
        assert_eq!(saved["maxWorktrees"], 8);
    }

    #[test]
    fn invalid_number_is_rejected_without_saving() {
        let _lock = data_dir();
        let mut ed = SettingsEditor::new(&Settings::default());
        goto(&mut ed, "pollingIntervalMs");
        ed.on_key(Key::Enter);
        for _ in 0..10 {
            ed.on_key(Key::Backspace);
        }
        ed.on_key(Key::Char('5'));
        assert!(matches!(ed.on_key(Key::Enter), Outcome::None));
        assert!(ed.error.is_some());
    }

    #[test]
    fn context_window_is_typed_in_k_or_m_and_steps_through_presets() {
        let _lock = data_dir();
        let mut ed = SettingsEditor::new(&Settings::default());
        goto(&mut ed, "contextWindowTokens");
        assert_eq!(ed.display(ed.field()), "1.0M");

        ed.on_key(Key::Enter);
        assert_eq!(ed.editing.as_deref(), Some("1.0M"));
        for _ in 0..4 {
            ed.on_key(Key::Backspace);
        }
        for c in "128k".chars() {
            ed.on_key(Key::Char(c));
        }
        let Outcome::Changed(s) = ed.on_key(Key::Enter) else { panic!("expected change") };
        assert_eq!(s.context_window_tokens, 128_000);
        assert_eq!(ed.display(ed.field()), "128k");

        let Outcome::Changed(s) = ed.on_key(Key::Right) else { panic!() };
        assert_eq!(s.context_window_tokens, 200_000);
        let Outcome::Changed(s) = ed.on_key(Key::Left) else { panic!() };
        assert_eq!(s.context_window_tokens, 128_000);

        // A custom value moves to the nearest preset in that direction.
        ed.values.insert("contextWindowTokens".into(), Value::from(300_000));
        let Outcome::Changed(s) = ed.on_key(Key::Left) else { panic!() };
        assert_eq!(s.context_window_tokens, 256_000);

        ed.on_key(Key::Enter);
        for _ in 0..10 {
            ed.on_key(Key::Backspace);
        }
        for c in "abc".chars() {
            ed.on_key(Key::Char(c));
        }
        assert!(matches!(ed.on_key(Key::Enter), Outcome::None));
        assert!(ed.error.as_deref().is_some_and(|e| e.contains("128k")));
    }

    #[test]
    fn sort_editor_reorders_toggles_and_flips() {
        let _lock = data_dir();
        let mut ed = SettingsEditor::new(&Settings::default());
        goto(&mut ed, "worktreeSort");
        ed.on_key(Key::Enter);
        ed.on_key(Key::Char('J')); // move "isMain" down
        ed.on_key(Key::Char('d')); // flip it
        ed.on_key(Key::Char(' ')); // disable it
        let Outcome::Changed(s) = ed.on_key(Key::Enter) else { panic!() };
        assert_eq!(s.worktree_sort[0].key, SortKey::LinearTicket);
        assert_eq!(s.worktree_sort[1].key, SortKey::IsMain);
        assert_eq!(s.worktree_sort[1].direction, Direction::Desc);
        assert!(!s.worktree_sort[1].enabled);
    }

    #[test]
    fn choosing_a_sound_previews_it() {
        let _lock = data_dir();
        let mut app = crate::app::App::new(Settings::default());
        app.on_key(Key::Char('s'));
        let ed = app.settings_editor.as_mut().unwrap();
        goto(ed, "audioDoneSound");
        app.on_key(Key::Right); // Funk → Glass
        assert_eq!(app.sounds, vec!["Glass".to_string()]);
    }

    #[test]
    fn theme_row_cycles_builtins_and_custom_themes() {
        let (_lock, dir) = data_dir();
        let _ = std::fs::remove_dir_all(dir.join("themes"));
        let spec = themes::ThemeSpec { name: "zz-mine".into(), base: Some("nord".into()), colors: Default::default() };
        themes::save_custom_in(&dir.join("themes"), &spec).unwrap();
        let mut ed = SettingsEditor::new(&Settings::parse(r#"{"theme":"amber-phosphor"}"#));
        goto(&mut ed, "theme");
        let Outcome::Changed(s) = ed.on_key(Key::Right) else { panic!() };
        assert_eq!(s.theme, "zz-mine", "custom themes follow the built-ins");
        let Outcome::Changed(s) = ed.on_key(Key::Right) else { panic!() };
        assert_eq!(s.theme, "ansi", "wraps around");
        let Outcome::Changed(s) = ed.on_key(Key::Left) else { panic!() };
        assert_eq!(s.theme, "zz-mine");
        std::fs::remove_dir_all(dir.join("themes")).unwrap();
    }

    #[test]
    fn builder_saves_the_theme_file_and_selects_it() {
        let (_lock, dir) = data_dir();
        let _ = std::fs::remove_dir_all(dir.join("themes"));
        let mut ed = SettingsEditor::new(&Settings::parse(r#"{"theme":"dracula"}"#));
        goto(&mut ed, "themeBuilder");
        assert!(matches!(ed.on_key(Key::Enter), Outcome::None));
        let b = ed.builder.as_ref().expect("builder opens");
        assert_eq!((b.name.as_str(), b.base.as_str()), ("dracula-custom", "dracula"));

        ed.on_key(Key::Char('#'));
        for _ in 0..7 {
            ed.on_key(Key::Backspace);
        }
        for c in "#ff0000".chars() {
            ed.on_key(Key::Char(c));
        }
        ed.on_key(Key::Enter);
        let Outcome::Changed(s) = ed.on_key(Key::CtrlS) else { panic!("expected save") };
        assert_eq!(s.theme, "dracula-custom");
        assert!(ed.builder.is_none());
        let saved = themes::read_custom_in(&dir.join("themes"), "dracula-custom").unwrap();
        assert_eq!(saved.base.as_deref(), Some("dracula"));
        assert_eq!(saved.get(themes::Token::Text), themes::ThemeColor::Rgb(0xff, 0, 0));

        // Reopening edits the saved theme in place.
        let mut ed = SettingsEditor::new(&s);
        goto(&mut ed, "themeBuilder");
        ed.on_key(Key::Enter);
        let b = ed.builder.as_ref().unwrap();
        assert_eq!(b.name, "dracula-custom");
        assert_eq!(b.color(themes::Token::Text), (themes::ThemeColor::Rgb(0xff, 0, 0), true));
        std::fs::remove_dir_all(dir.join("themes")).unwrap();
    }

    #[test]
    fn key_bindings_page_saves_only_changed_keys() {
        let (_lock, dir) = data_dir();
        std::fs::write(dir.join("settings.json"), r#"{"showIdeStatus":true}"#).unwrap();
        let mut ed = SettingsEditor::new(&Settings::default());
        goto(&mut ed, "keyBindings");
        assert_eq!(ed.display(ed.field()), "defaults");
        ed.on_key(Key::Enter);
        let keys = ed.keys.as_mut().expect("page opens");
        keys.cursor = crate::keymap::Command::ALL.iter().position(|c| *c == crate::keymap::Command::NewWorktree).unwrap();
        ed.on_key(Key::Enter);
        let Outcome::Changed(s) = ed.on_key(Key::Char('N')) else { panic!("expected save") };
        assert_eq!(s.key_bindings.get("newWorktree").map(String::as_str), Some("N"));
        let saved: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap()).unwrap();
        assert_eq!(saved["keyBindings"], serde_json::json!({ "newWorktree": "N" }));
        assert_eq!(saved["showIdeStatus"], true, "unknown keys survive");

        // The app keeps the page open across the change and applies it live.
        let mut app = crate::app::App::new(Settings::default());
        app.settings_editor = Some(ed);
        app.set_settings(*s);
        assert!(app.settings_editor.as_ref().is_some_and(|e| e.keys.is_some()));
        assert_eq!(app.keymap.command(Key::Char('N')), Some(crate::keymap::Command::NewWorktree));
        let ed = app.settings_editor.as_mut().unwrap();
        ed.on_key(Key::Char('x'));
        ed.on_key(Key::Esc);
        assert!(ed.keys.is_none());
        let saved: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap()).unwrap();
        assert_eq!(saved["keyBindings"], serde_json::json!({}));
    }

    #[test]
    fn groups_page_saves_both_keys() {
        let (_lock, dir) = data_dir();
        std::fs::write(dir.join("settings.json"), r#"{"showIdeStatus":true}"#).unwrap();
        let mut ed = SettingsEditor::only(&Settings::default(), REVIEW_FILTERS);
        goto(&mut ed, "reviewGroups");
        assert_eq!(ed.display(ed.field()), "no groups");
        assert!(matches!(ed.on_key(Key::Enter), Outcome::LoadPeople));
        let Outcome::Changed(s) = ed.on_key(Key::Char('a')) else { panic!("expected save") };
        assert_eq!(s.review_groups.len(), 1);
        ed.on_key(Key::Esc); // the form
        ed.on_key(Key::Char('j')); // the default group
        let Outcome::Changed(s) = ed.on_key(Key::Char('b')) else { panic!() };
        assert!(!s.review_default_group.badge);
        let saved: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap()).unwrap();
        assert_eq!(saved["reviewGroups"][0]["asked"], "any");
        assert_eq!(saved["reviewDefaultGroup"]["badge"], false);
        assert_eq!(saved["showIdeStatus"], true, "unknown keys survive");
        ed.on_key(Key::Esc);
        assert!(ed.groups.is_none());
        assert_eq!(ed.display(ed.field()), "1 group");
    }

    #[test]
    fn groups_page_stays_open_across_its_saves_in_the_app() {
        let (_lock, _) = data_dir();
        let mut app = crate::app::App::new(Settings::default());
        app.settings_editor = Some(SettingsEditor::only(&Settings::default(), REVIEW_FILTERS));
        goto(app.settings_editor.as_mut().unwrap(), "reviewGroups");
        app.on_key(Key::Enter);
        app.on_key(Key::Char('a'));
        let ed = app.settings_editor.as_ref().unwrap();
        let g = ed.groups.as_ref().expect("the page stays open");
        assert!(g.form.is_some(), "on the new group's form");
        assert_eq!(app.settings.review_groups.len(), 1);
    }

    #[test]
    fn review_resets_ask_then_clear_filters_or_groups() {
        let (_lock, dir) = data_dir();
        let saved = r#"{"reviewRequestsScope":"teams","reviewBadgeTotal":true,"showIdeStatus":true,
            "reviewDefaultGroup":{"draft":"ready","sort":"oldest"},"reviewGroups":[{"name":"Mine","asked":"you"}]}"#;
        std::fs::write(dir.join("settings.json"), saved).unwrap();
        let mut ed = SettingsEditor::only(&Settings::parse(saved), REVIEW_FILTERS);
        goto(&mut ed, "resetReviewGroups");
        assert_eq!(ed.display(ed.field()), "⏎ remove 1");
        // Anything but y cancels.
        assert!(matches!(ed.on_key(Key::Enter), Outcome::None));
        assert_eq!(ed.confirm, Some("resetReviewGroups"));
        assert!(matches!(ed.on_key(Key::Char('n')), Outcome::None));
        assert!(ed.confirm.is_none());
        ed.on_key(Key::Enter);
        let Outcome::Changed(s) = ed.on_key(Key::Char('y')) else { panic!("expected a change") };
        assert!(s.review_groups.is_empty());
        assert_eq!(s.review_default_group.draft, "ready", "filters kept");

        goto(&mut ed, "resetReviewFilters");
        ed.on_key(Key::Enter);
        let Outcome::Changed(s) = ed.on_key(Key::Char('y')) else { panic!("expected a change") };
        assert_eq!(s.review_requests_scope, "all");
        assert!(!s.review_badge_total);
        assert_eq!(s.review_default_group, am_core::review_groups::ReviewGroup::default());
        let file: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap()).unwrap();
        assert_eq!(file["showIdeStatus"], true, "unknown keys survive");
        assert_eq!(file["reviewGroups"], serde_json::json!([]));
    }

    #[test]
    fn secret_is_masked() {
        let s = Settings { linear_api_key: "lin_api_abcdef1234".into(), ..Settings::default() };
        let ed = SettingsEditor::new(&s);
        let f = FIELDS.iter().find(|f| f.key == "linearApiKey").unwrap();
        assert_eq!(ed.display(f), "••••1234");
    }

    #[test]
    fn plugin_page_toggles_hooks_and_the_default() {
        let (_lock, dir) = data_dir();
        // Hooks go to $HOME: point it at the temp dir for this test.
        let home = dir.join("home");
        std::fs::create_dir_all(home.join(".gemini")).unwrap();
        let real_home = std::env::var_os("HOME");
        // SAFETY: settings tests hold LOCK; HOME is restored below.
        unsafe { std::env::set_var("HOME", &home) };

        let mut ed = SettingsEditor::new(&Settings::parse(r#"{"defaultAgent":"gemini"}"#));
        goto(&mut ed, "agentPlugins");
        assert_eq!(ed.display(ed.field()), "7 of 7 on · default gemini");
        ed.on_key(Key::Enter);
        let rows = &ed.plugins.as_ref().unwrap().rows;
        assert_eq!(
            rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            ["claude", "gemini", "codex", "qwen", "droid", "copilot", "cursor"]
        );
        assert!(rows.iter().all(|r| r.enabled && !r.hooks));

        // Install Gemini's hooks, then switch it off: hooks go, and so does the default.
        ed.on_key(Key::Char('j'));
        ed.on_key(Key::Char('i'));
        let gemini = am_core::harness::HarnessKind::GEMINI;
        assert!(am_core::hooks_installer::is_hooks_installed(gemini));
        let Outcome::Changed(s) = ed.on_key(Key::Char(' ')) else { panic!("expected change") };
        assert_eq!(s.disabled_agents, ["gemini"]);
        assert_eq!(s.default_agent, "claude");
        assert!(!am_core::hooks_installer::is_hooks_installed(gemini));
        let pl = ed.plugins.as_ref().unwrap();
        assert_eq!(pl.selected().unwrap().id, "gemini");
        assert!(!pl.selected().unwrap().enabled);
        assert!(pl.message.as_ref().is_some_and(|(ok, m)| *ok && m.contains("hooks removed")));

        // On again (installs, the CLI is present), and make it the default.
        ed.on_key(Key::Char(' '));
        assert!(am_core::hooks_installer::is_hooks_installed(gemini));
        let Outcome::Changed(s) = ed.on_key(Key::Char('d')) else { panic!() };
        assert_eq!((s.default_agent.as_str(), s.disabled_agents.len()), ("gemini", 0));
        ed.on_key(Key::Esc);
        assert!(ed.plugins.is_none());

        match real_home {
            // SAFETY: as above.
            Some(h) => unsafe { std::env::set_var("HOME", h) },
            None => unsafe { std::env::remove_var("HOME") },
        }
    }
}
