//! Slim settings editor (`s`). Edits `settings.json` key by key so unknown
//! keys (written by the TS app or a newer version) survive. `e` opens the
//! whole file in $EDITOR for anything not listed here.

use am_core::settings::{Direction, SortCriterion, SortKey, Settings};
use am_core::themes;
use serde_json::{Map, Value};

use crate::app::Key;
use crate::theme_builder::{BuilderOutcome, ThemeBuilder};

#[derive(Debug, Clone, Copy)]
pub enum Kind {
    Bool,
    Choice(&'static [&'static str]),
    Number { step: u64, min: u64 },
    Text { secret: bool },
    Sort,
    /// Built-in and custom themes, listed at runtime.
    Theme,
    /// Opens the theme builder.
    ThemeBuilder,
}

pub struct Field {
    pub key: &'static str,
    pub label: &'static str,
    pub help: &'static str,
    pub section: &'static str,
    pub kind: Kind,
}

const SOUNDS: &[&str] =
    &["Basso", "Blow", "Bottle", "Frog", "Funk", "Glass", "Hero", "Morse", "Ping", "Pop", "Purr", "Sosumi", "Submarine", "Tink"];

pub const FIELDS: &[Field] = &[
    Field { key: "ide", label: "Open in", help: "Which editor/IDE Enter opens worktrees in", section: "General", kind: Kind::Choice(&["cursor", "vscode", "terminal"]) },
    Field { key: "resumeLastSession", label: "Resume last session on open", help: "Enter also resumes the worktree's most recent Claude session", section: "General", kind: Kind::Bool },
    Field { key: "mouseEnabled", label: "Mouse", help: "Click, double-click and scroll; off restores the terminal's own text selection", section: "General", kind: Kind::Bool },
    Field { key: "startupScriptMode", label: "Run startup script in", help: "pane: output in the Jobs pane; window: a new terminal window", section: "General", kind: Kind::Choice(&["pane", "window"]) },
    Field { key: "defaultBranchPrefix", label: "Branch prefix", help: "Prefix for new worktree branches", section: "General", kind: Kind::Text { secret: false } },
    Field { key: "defaultBaseBranch", label: "Base branch", help: "Default base branch for new worktrees", section: "General", kind: Kind::Text { secret: false } },
    Field { key: "pollingIntervalMs", label: "Git refresh (ms)", help: "Full git sweep interval; agent activity refreshes its worktree immediately", section: "General", kind: Kind::Number { step: 5000, min: 5000 } },
    Field { key: "theme", label: "Theme", help: "←→ to cycle; ansi follows your terminal palette", section: "Appearance", kind: Kind::Theme },
    Field { key: "themeBuilder", label: "Theme builder…", help: "Make your own theme from the current one: pick colours, preview live, save", section: "Appearance", kind: Kind::ThemeBuilder },
    Field { key: "worktreeSort", label: "Sort order", help: "Criteria applied in order; first difference wins", section: "Dashboard", kind: Kind::Sort },
    Field { key: "hideMainBranch", label: "Hide main branch", help: "Hide the main/master checkout from the list", section: "Dashboard", kind: Kind::Bool },
    Field { key: "hideMergedClosedPrs", label: "Hide merged/closed PRs", help: "Unless the agent session is open", section: "Dashboard", kind: Kind::Bool },
    Field { key: "hideIdleDoneAgents", label: "Hide idle/done agents", help: "Only show worktrees with active agents", section: "Dashboard", kind: Kind::Bool },
    Field { key: "hideWithoutLinearTicket", label: "Hide without Linear ticket", help: "Unless the session is open", section: "Dashboard", kind: Kind::Bool },
    Field { key: "showPrStatus", label: "Show PR column", help: "PR number and checks on each row", section: "Dashboard", kind: Kind::Bool },
    Field { key: "showLinearTicket", label: "Show ticket column", help: "Linear identifier on each row", section: "Dashboard", kind: Kind::Bool },
    Field { key: "showGitAheadBehind", label: "Show git column", help: "Ahead/behind/dirty counts", section: "Dashboard", kind: Kind::Bool },
    Field { key: "showLastCommit", label: "Show commit age", help: "Age of the last commit", section: "Dashboard", kind: Kind::Bool },
    Field { key: "showRunningProcesses", label: "Detect dev servers", help: "Show processes running inside each worktree", section: "Dashboard", kind: Kind::Bool },
    Field { key: "runningProcessFilter", label: "Process filter", help: "Only count processes whose command line contains this", section: "Dashboard", kind: Kind::Text { secret: false } },
    Field { key: "audioNotifications", label: "Sounds", help: "Play a sound when an agent is waiting or done", section: "Notifications", kind: Kind::Bool },
    Field { key: "checkForUpdates", label: "Check for updates", help: "Show a notice when a newer am release is on GitHub", section: "Notifications", kind: Kind::Bool },
    Field { key: "audioWaitingSound", label: "Waiting sound", help: "Played when an agent needs you", section: "Notifications", kind: Kind::Choice(SOUNDS) },
    Field { key: "audioDoneSound", label: "Done sound", help: "Played when an agent finishes", section: "Notifications", kind: Kind::Choice(SOUNDS) },
    Field { key: "ghPrStatus", label: "Fetch PRs", help: "GitHub PR and CI status via gh", section: "GitHub", kind: Kind::Bool },
    Field { key: "ghPollingIntervalMs", label: "PR refresh (ms)", help: "How often to fetch PR status", section: "GitHub", kind: Kind::Number { step: 30000, min: 30000 } },
    Field { key: "ghRefreshOnManual", label: "Refresh on r", help: "Include GitHub when refreshing manually", section: "GitHub", kind: Kind::Bool },
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
];

pub enum Outcome {
    None,
    Close,
    Changed(Box<Settings>),
    EditFile,
}

pub struct SettingsEditor {
    pub cursor: usize,
    pub editing: Option<String>,
    pub sort: Option<SortEditor>,
    pub builder: Option<ThemeBuilder>,
    pub values: Map<String, Value>,
    pub error: Option<String>,
}

impl SettingsEditor {
    pub fn new(settings: &Settings) -> SettingsEditor {
        let values = match serde_json::to_value(settings) {
            Ok(Value::Object(m)) => m,
            _ => Map::new(),
        };
        SettingsEditor { cursor: 0, editing: None, sort: None, builder: None, values, error: None }
    }

    /// A click on field `i`: selects it, or changes it (like ⏎) when it is
    /// already selected. Ignored while the sort editor or theme builder is open.
    pub fn click(&mut self, i: usize) -> Outcome {
        if self.sort.is_some() || self.builder.is_some() || i >= FIELDS.len() {
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
            Kind::ThemeBuilder => "⏎ open".into(),
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

    pub fn on_key(&mut self, key: Key) -> Outcome {
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
        self.error = None;
        match key {
            Key::Esc | Key::Char('q') | Key::Char('s') => return Outcome::Close,
            Key::Char('e') => return Outcome::EditFile,
            Key::Down | Key::Char('j') => self.cursor = (self.cursor + 1).min(FIELDS.len() - 1),
            Key::Up | Key::Char('k') => self.cursor = self.cursor.saturating_sub(1),
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
            Kind::Number { .. } => {
                self.editing = Some(self.values.get(f.key).map(|v| v.to_string()).unwrap_or_default());
                Outcome::None
            }
            Kind::Text { secret } => {
                let cur = self.values.get(f.key).and_then(Value::as_str).unwrap_or("");
                self.editing = Some(if secret { String::new() } else { cur.to_string() });
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
mod tests {
    use super::*;

    /// All editor tests share one temp AM_DATA_DIR (env is process-global),
    /// so they hold a lock while they touch settings.json.
    fn data_dir() -> (std::sync::MutexGuard<'static, ()>, &'static std::path::Path) {
        static DIR: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let d = DIR.get_or_init(|| tempfile::tempdir().unwrap());
        // SAFETY: only these tests read AM_DATA_DIR, and they hold LOCK.
        unsafe { std::env::set_var("AM_DATA_DIR", d.path()) };
        let _ = std::fs::remove_file(d.path().join("settings.json"));
        (guard, d.path())
    }

    fn goto(ed: &mut SettingsEditor, key: &str) {
        ed.cursor = FIELDS.iter().position(|f| f.key == key).unwrap();
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
    fn secret_is_masked() {
        let s = Settings { linear_api_key: "lin_api_abcdef1234".into(), ..Settings::default() };
        let ed = SettingsEditor::new(&s);
        let f = FIELDS.iter().find(|f| f.key == "linearApiKey").unwrap();
        assert_eq!(ed.display(f), "••••1234");
    }
}
