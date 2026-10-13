//! Settings → Agent plugins: one row per registered agent plugin (built-in
//! and manifest), to switch it on/off, (re)install its hooks and pick the
//! default agent, plus training mode: `T` (or the last row) trains a new
//! harness, `e` retrains a manifest plugin, `p` publishes a draft and `x`
//! removes one (both asked to confirm). Pure state and keys; `settings_ui`
//! performs the effects and hands back fresh rows.

use am_core::harness::manifest::PluginStatus;
use am_core::harness::{self, Origin};
use am_core::settings::Settings;

use crate::app::Key;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginRow {
    pub id: String,
    pub name: String,
    /// "built-in" or the manifest's file name.
    pub source: String,
    pub enabled: bool,
    pub hooks: bool,
    /// The CLI has been used on this machine.
    pub present: bool,
    /// Its hook settings file, `~`-relative.
    pub settings_path: String,
    pub note: Option<String>,
    pub builtin: bool,
    pub draft: bool,
    /// Distinct problems recorded while it was used (`plugin_issues`).
    pub issues: usize,
    /// Hook events captured for it (drafts capture every event).
    pub captures: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginOutcome {
    Stay,
    Close,
    SetEnabled(String, bool),
    InstallHooks(String),
    SetDefault(String),
    /// Train a new harness plugin for the harness started with this command.
    TrainNew(String),
    /// Retrain a manifest plugin.
    Edit(String),
    Publish(String),
    Remove(String),
}

/// An action waiting for `y`/Enter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Confirm {
    Publish(String),
    Remove(String),
}

#[derive(Debug, Clone)]
pub struct PluginEditor {
    pub rows: Vec<PluginRow>,
    pub cursor: usize,
    pub default_agent: String,
    /// Manifests that failed to load (`file: reason`).
    pub errors: Vec<String>,
    /// Result of the last action.
    pub message: Option<(bool, String)>,
    /// Where manifest plugins go, for the hint line (`~`-relative).
    pub plugins_dir: String,
    /// The "train a new harness plugin" command being typed.
    pub train: Option<String>,
    pub confirm: Option<Confirm>,
}

/// Snapshot every registered plugin's state (reads the hook settings files).
pub fn load_rows(settings: &Settings) -> Vec<PluginRow> {
    let issues = am_core::db::open().ok().and_then(|c| am_core::db::plugin_issue_counts(&c).ok()).unwrap_or_default();
    harness::all()
        .iter()
        .map(|&k| {
            let p = k.provider();
            PluginRow {
                id: k.id().to_string(),
                name: p.name().to_string(),
                source: match p.origin() {
                    Origin::BuiltIn => "built-in".into(),
                    Origin::Plugin(path) => path.file_name().unwrap_or_default().to_string_lossy().into_owned(),
                },
                enabled: harness::is_enabled(k, settings),
                hooks: am_core::hooks_installer::is_hooks_installed(k),
                present: k == am_core::harness::HarnessKind::CLAUDE || p.is_present(),
                settings_path: am_core::hooks_installer::display_path(p),
                note: p.notes().map(str::to_owned),
                builtin: k.is_builtin(),
                draft: p.status() == PluginStatus::Draft,
                issues: issues.get(k.id()).copied().unwrap_or(0),
                captures: if k.is_builtin() { 0 } else { am_core::harness::capture::list(k.id()).len() },
            }
        })
        .collect()
}

impl PluginEditor {
    pub fn new(rows: Vec<PluginRow>, default_agent: &str, errors: Vec<String>, plugins_dir: String) -> PluginEditor {
        PluginEditor {
            rows,
            cursor: 0,
            default_agent: default_agent.to_string(),
            errors,
            message: None,
            plugins_dir,
            train: None,
            confirm: None,
        }
    }

    /// The selected plugin; `None` on the "train a new harness plugin" row.
    pub fn selected(&self) -> Option<&PluginRow> {
        self.rows.get(self.cursor)
    }

    /// The cursor is on the trailing "＋ Train a new harness plugin…" row.
    pub fn on_train_row(&self) -> bool {
        self.cursor == self.rows.len()
    }

    pub fn on_key(&mut self, key: Key) -> PluginOutcome {
        self.message = None;
        if let Some(cmd) = &mut self.train {
            return match key {
                Key::Esc => {
                    self.train = None;
                    PluginOutcome::Stay
                }
                Key::Enter if cmd.trim().is_empty() => self.refuse("Type the command that starts the harness, e.g. cursor-agent".into()),
                Key::Enter => {
                    let cmd = cmd.trim().to_owned();
                    self.train = None;
                    PluginOutcome::TrainNew(cmd)
                }
                Key::Backspace => {
                    cmd.pop();
                    PluginOutcome::Stay
                }
                Key::Char(c) => {
                    cmd.push(c);
                    PluginOutcome::Stay
                }
                _ => PluginOutcome::Stay,
            };
        }
        if let Some(c) = self.confirm.take() {
            return match (key, c) {
                (Key::Enter | Key::Char('y'), Confirm::Publish(id)) => PluginOutcome::Publish(id),
                (Key::Enter | Key::Char('y'), Confirm::Remove(id)) => PluginOutcome::Remove(id),
                _ => PluginOutcome::Stay,
            };
        }
        match key {
            Key::Down | Key::Char('j') => {
                self.cursor = (self.cursor + 1).min(self.rows.len());
                return PluginOutcome::Stay;
            }
            Key::Up | Key::Char('k') => {
                self.cursor = self.cursor.saturating_sub(1);
                return PluginOutcome::Stay;
            }
            Key::Char('T') => {
                self.train = Some(String::new());
                return PluginOutcome::Stay;
            }
            Key::Enter if self.on_train_row() => {
                self.train = Some(String::new());
                return PluginOutcome::Stay;
            }
            _ => {}
        }
        let Some(row) = self.rows.get(self.cursor) else {
            return if matches!(key, Key::Esc | Key::Char('q')) { PluginOutcome::Close } else { PluginOutcome::Stay };
        };
        match key {
            Key::Esc | Key::Enter | Key::Char('q') => PluginOutcome::Close,
            Key::Up | Key::Char('k') => {
                self.cursor = self.cursor.saturating_sub(1);
                PluginOutcome::Stay
            }
            Key::Char(' ') => PluginOutcome::SetEnabled(row.id.clone(), !row.enabled),
            Key::Char('i') if row.enabled => PluginOutcome::InstallHooks(row.id.clone()),
            Key::Char('i') => self.refuse(format!("{} is off; space turns it on", row.name)),
            Key::Char('d') if row.enabled => PluginOutcome::SetDefault(row.id.clone()),
            Key::Char('d') => self.refuse(format!("{} is off; it can't be the default", row.name)),
            Key::Char('e' | 'p' | 'x') if row.builtin => self.refuse(format!("{} is built in; T trains a new harness plugin", row.name)),
            Key::Char('e') => PluginOutcome::Edit(row.id.clone()),
            Key::Char('p') if !row.draft => self.refuse(format!("{} is already published; e retrains it", row.name)),
            Key::Char('p') => {
                self.confirm = Some(Confirm::Publish(row.id.clone()));
                PluginOutcome::Stay
            }
            Key::Char('x') => {
                self.confirm = Some(Confirm::Remove(row.id.clone()));
                PluginOutcome::Stay
            }
            _ => PluginOutcome::Stay,
        }
    }

    fn refuse(&mut self, msg: String) -> PluginOutcome {
        self.message = Some((false, msg));
        PluginOutcome::Stay
    }

    /// After an effect: fresh rows, the cursor kept on the same plugin.
    pub fn refresh(&mut self, rows: Vec<PluginRow>, default_agent: &str, message: (bool, String)) {
        let id = self.selected().map(|r| r.id.clone());
        self.rows = rows;
        self.cursor = id.and_then(|id| self.rows.iter().position(|r| r.id == id)).unwrap_or(self.cursor.min(self.rows.len()));
        self.default_agent = default_agent.to_string();
        self.message = Some(message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, enabled: bool) -> PluginRow {
        PluginRow {
            id: id.into(),
            name: format!("{id} CLI"),
            source: "built-in".into(),
            enabled,
            hooks: enabled,
            present: true,
            settings_path: format!("~/.{id}/settings.json"),
            note: None,
            builtin: id != "toycli",
            draft: id == "toycli",
            issues: 0,
            captures: 0,
        }
    }

    #[test]
    fn keys_map_to_effects() {
        let mut ed = PluginEditor::new(vec![row("claude", true), row("gemini", false)], "claude", vec![], String::new());
        assert_eq!(ed.on_key(Key::Char(' ')), PluginOutcome::SetEnabled("claude".into(), false));
        assert_eq!(ed.on_key(Key::Char('i')), PluginOutcome::InstallHooks("claude".into()));
        assert_eq!(ed.on_key(Key::Char('d')), PluginOutcome::SetDefault("claude".into()));
        ed.on_key(Key::Char('j'));
        ed.on_key(Key::Char('j'));
        assert_eq!(ed.cursor, 2, "the train row follows the plugins");
        ed.on_key(Key::Char('k'));
        // A disabled plugin can only be switched on.
        assert_eq!(ed.on_key(Key::Char('i')), PluginOutcome::Stay);
        assert!(ed.message.as_ref().is_some_and(|(ok, m)| !ok && m.contains("is off")));
        assert_eq!(ed.on_key(Key::Char('d')), PluginOutcome::Stay);
        assert_eq!(ed.on_key(Key::Char(' ')), PluginOutcome::SetEnabled("gemini".into(), true));
        assert_eq!(ed.on_key(Key::Esc), PluginOutcome::Close);
    }

    #[test]
    fn refresh_keeps_the_cursor_on_the_same_plugin() {
        let mut ed = PluginEditor::new(vec![row("claude", true), row("gemini", true)], "claude", vec![], String::new());
        ed.cursor = 1;
        ed.refresh(vec![row("gemini", false), row("claude", true)], "claude", (true, "ok".into()));
        assert_eq!(ed.selected().unwrap().id, "gemini");
        assert_eq!(ed.message, Some((true, "ok".into())));
    }

    #[test]
    fn training_keys() {
        let mut ed = PluginEditor::new(vec![row("claude", true), row("toycli", true)], "claude", vec![], String::new());
        // Built-ins can't be retrained, published or removed.
        assert_eq!(ed.on_key(Key::Char('e')), PluginOutcome::Stay);
        assert!(ed.message.as_ref().is_some_and(|(_, m)| m.contains("built in")));
        ed.on_key(Key::Char('j'));
        assert_eq!(ed.on_key(Key::Char('e')), PluginOutcome::Edit("toycli".into()));
        // Publish and remove ask first; any other key cancels.
        assert_eq!(ed.on_key(Key::Char('p')), PluginOutcome::Stay);
        assert_eq!(ed.confirm, Some(Confirm::Publish("toycli".into())));
        assert_eq!(ed.on_key(Key::Char('y')), PluginOutcome::Publish("toycli".into()));
        ed.on_key(Key::Char('x'));
        assert_eq!(ed.on_key(Key::Char('n')), PluginOutcome::Stay);
        assert_eq!(ed.confirm, None);
        // T, or Enter on the last row, types the command to train.
        ed.on_key(Key::Char('j'));
        assert!(ed.on_train_row());
        ed.on_key(Key::Enter);
        assert_eq!(ed.on_key(Key::Enter), PluginOutcome::Stay, "empty command refused");
        for c in "my cli".chars() {
            ed.on_key(Key::Char(c));
        }
        ed.on_key(Key::Backspace);
        assert_eq!(ed.on_key(Key::Enter), PluginOutcome::TrainNew("my cl".into()));
        assert_eq!(ed.train, None);
        ed.on_key(Key::Char('T'));
        assert_eq!(ed.on_key(Key::Esc), PluginOutcome::Stay);
        assert_eq!(ed.train, None);
        assert_eq!(ed.on_key(Key::Esc), PluginOutcome::Close);
    }
}
