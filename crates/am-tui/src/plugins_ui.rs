//! Settings → Agent plugins: one row per registered agent plugin (built-in
//! and manifest), to switch it on/off, (re)install its hooks and pick the
//! default agent. Pure state and keys; `settings_ui` performs the effects
//! and hands back fresh rows.

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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginOutcome {
    Stay,
    Close,
    SetEnabled(String, bool),
    InstallHooks(String),
    SetDefault(String),
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
}

/// Snapshot every registered plugin's state (reads the hook settings files).
pub fn load_rows(settings: &Settings) -> Vec<PluginRow> {
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
            }
        })
        .collect()
}

impl PluginEditor {
    pub fn new(rows: Vec<PluginRow>, default_agent: &str, errors: Vec<String>, plugins_dir: String) -> PluginEditor {
        PluginEditor { rows, cursor: 0, default_agent: default_agent.to_string(), errors, message: None, plugins_dir }
    }

    pub fn selected(&self) -> Option<&PluginRow> {
        self.rows.get(self.cursor)
    }

    pub fn on_key(&mut self, key: Key) -> PluginOutcome {
        self.message = None;
        let Some(row) = self.rows.get(self.cursor) else {
            return if matches!(key, Key::Esc | Key::Enter | Key::Char('q')) { PluginOutcome::Close } else { PluginOutcome::Stay };
        };
        match key {
            Key::Esc | Key::Enter | Key::Char('q') => PluginOutcome::Close,
            Key::Down | Key::Char('j') => {
                self.cursor = (self.cursor + 1).min(self.rows.len() - 1);
                PluginOutcome::Stay
            }
            Key::Up | Key::Char('k') => {
                self.cursor = self.cursor.saturating_sub(1);
                PluginOutcome::Stay
            }
            Key::Char(' ') => PluginOutcome::SetEnabled(row.id.clone(), !row.enabled),
            Key::Char('i') if row.enabled => PluginOutcome::InstallHooks(row.id.clone()),
            Key::Char('i') => self.refuse(format!("{} is off; space turns it on", row.name)),
            Key::Char('d') if row.enabled => PluginOutcome::SetDefault(row.id.clone()),
            Key::Char('d') => self.refuse(format!("{} is off; it can't be the default", row.name)),
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
        self.cursor = id.and_then(|id| self.rows.iter().position(|r| r.id == id)).unwrap_or(0);
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
        assert_eq!(ed.cursor, 1);
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
}
