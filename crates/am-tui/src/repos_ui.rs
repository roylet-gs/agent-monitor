//! Settings → Repositories…: the tracked repositories, to add one (folder
//! browser), stop tracking one, and edit or remove its startup and review
//! scripts. Pure
//! state and keys; `settings_ui` performs the effects and hands back fresh
//! rows (port of the 1.x Settings "Repositories" section).

use std::path::Path;

use am_core::worktrees::ScriptKind;

use crate::app::Key;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoRow {
    pub id: String,
    pub name: String,
    /// `~`-relative.
    pub path: String,
    /// The directory still exists.
    pub exists: bool,
    pub worktrees: usize,
    /// Has a startup script (`scripts/<id>.sh`).
    pub script: bool,
    /// Has a review script (`scripts/<id>.review.sh`), run by "pull + run
    /// script" on the review page.
    pub review_script: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoOutcome {
    Stay,
    Close,
    Add,
    Remove(String),
    EditScript(String, ScriptKind),
    RemoveScript(String, ScriptKind),
}

#[derive(Debug, Clone)]
pub struct RepoEditor {
    pub rows: Vec<RepoRow>,
    pub cursor: usize,
    /// Repo id waiting for `y` to stop tracking it.
    pub confirm_remove: Option<String>,
    /// Result of the last action.
    pub message: Option<(bool, String)>,
}

/// Snapshot the tracked repositories (most recently used first).
pub fn load_rows() -> Vec<RepoRow> {
    let conn = match am_core::db::open() {
        Ok(c) => c,
        Err(e) => {
            am_core::warn!("repo", "Couldn't open the DB to list repositories: {e}");
            return Vec::new();
        }
    };
    am_core::db::repositories(&conn)
        .unwrap_or_default()
        .into_iter()
        .map(|r| RepoRow {
            worktrees: am_core::db::worktrees_for_repo(&conn, &r.id).map(|w| w.len()).unwrap_or(0),
            script: am_core::worktrees::has_script(&r.id),
            review_script: ScriptKind::Review.exists(&r.id),
            exists: Path::new(&r.path).is_dir(),
            path: am_core::hooks_installer::tilde(Path::new(&r.path)),
            id: r.id,
            name: r.name,
        })
        .collect()
}

impl RepoEditor {
    pub fn new(rows: Vec<RepoRow>) -> RepoEditor {
        RepoEditor { rows, cursor: 0, confirm_remove: None, message: None }
    }

    pub fn selected(&self) -> Option<&RepoRow> {
        self.rows.get(self.cursor)
    }

    pub fn on_key(&mut self, key: Key) -> RepoOutcome {
        self.message = None;
        if let Some(id) = self.confirm_remove.take() {
            return if key == Key::Char('y') { RepoOutcome::Remove(id) } else { RepoOutcome::Stay };
        }
        match key {
            Key::Esc | Key::Char('q') => return RepoOutcome::Close,
            Key::Char('a') => return RepoOutcome::Add,
            Key::Down | Key::Char('j') => self.cursor = (self.cursor + 1).min(self.rows.len().saturating_sub(1)),
            Key::Up | Key::Char('k') => self.cursor = self.cursor.saturating_sub(1),
            _ => {}
        }
        let Some(row) = self.rows.get(self.cursor) else { return RepoOutcome::Stay };
        match key {
            Key::Char('x') => self.confirm_remove = Some(row.id.clone()),
            Key::Char('s') | Key::Enter => return RepoOutcome::EditScript(row.id.clone(), ScriptKind::Startup),
            Key::Char('S') if row.script => return RepoOutcome::RemoveScript(row.id.clone(), ScriptKind::Startup),
            Key::Char('S') => self.message = Some((false, format!("{} has no startup script", row.name))),
            Key::Char('r') => return RepoOutcome::EditScript(row.id.clone(), ScriptKind::Review),
            Key::Char('R') if row.review_script => return RepoOutcome::RemoveScript(row.id.clone(), ScriptKind::Review),
            Key::Char('R') => self.message = Some((false, format!("{} has no review script", row.name))),
            _ => {}
        }
        RepoOutcome::Stay
    }

    /// Move the cursor to the repo called `name` (just added), if listed.
    pub fn select_name(&mut self, name: &str) {
        if let Some(i) = self.rows.iter().position(|r| r.name == name) {
            self.cursor = i;
        }
    }

    /// After an effect: fresh rows, the cursor kept on the same repo.
    pub fn refresh(&mut self, rows: Vec<RepoRow>, message: Option<(bool, String)>) {
        let id = self.selected().map(|r| r.id.clone());
        let old = self.cursor;
        self.rows = rows;
        self.cursor = id
            .and_then(|id| self.rows.iter().position(|r| r.id == id))
            .unwrap_or(old.min(self.rows.len().saturating_sub(1)));
        self.message = message;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, script: bool) -> RepoRow {
        RepoRow { id: id.into(), name: format!("{id}-name"), path: format!("~/{id}"), exists: true, worktrees: 2, script, review_script: script }
    }

    #[test]
    fn keys_map_to_effects() {
        let mut ed = RepoEditor::new(vec![row("a", true), row("b", false)]);
        assert_eq!(ed.on_key(Key::Char('s')), RepoOutcome::EditScript("a".into(), ScriptKind::Startup));
        assert_eq!(ed.on_key(Key::Enter), RepoOutcome::EditScript("a".into(), ScriptKind::Startup));
        assert_eq!(ed.on_key(Key::Char('S')), RepoOutcome::RemoveScript("a".into(), ScriptKind::Startup));
        assert_eq!(ed.on_key(Key::Char('r')), RepoOutcome::EditScript("a".into(), ScriptKind::Review));
        assert_eq!(ed.on_key(Key::Char('R')), RepoOutcome::RemoveScript("a".into(), ScriptKind::Review));
        assert_eq!(ed.on_key(Key::Char('a')), RepoOutcome::Add);
        ed.on_key(Key::Char('j'));
        ed.on_key(Key::Char('j'));
        assert_eq!(ed.cursor, 1);
        // No script to remove.
        assert_eq!(ed.on_key(Key::Char('S')), RepoOutcome::Stay);
        assert!(ed.message.as_ref().is_some_and(|(ok, m)| !ok && m.contains("no startup script")));
        assert_eq!(ed.on_key(Key::Char('R')), RepoOutcome::Stay);
        assert!(ed.message.as_ref().is_some_and(|(ok, m)| !ok && m.contains("no review script")));
        assert_eq!(ed.on_key(Key::Esc), RepoOutcome::Close);
    }

    #[test]
    fn remove_asks_first() {
        let mut ed = RepoEditor::new(vec![row("a", false)]);
        assert_eq!(ed.on_key(Key::Char('x')), RepoOutcome::Stay);
        assert_eq!(ed.confirm_remove.as_deref(), Some("a"));
        // Anything but y cancels.
        assert_eq!(ed.on_key(Key::Char('n')), RepoOutcome::Stay);
        assert_eq!(ed.confirm_remove, None);
        ed.on_key(Key::Char('x'));
        assert_eq!(ed.on_key(Key::Char('y')), RepoOutcome::Remove("a".into()));
    }

    #[test]
    fn empty_list_can_still_add_and_close() {
        let mut ed = RepoEditor::new(vec![]);
        assert_eq!(ed.on_key(Key::Char('x')), RepoOutcome::Stay);
        assert_eq!(ed.on_key(Key::Char('s')), RepoOutcome::Stay);
        assert_eq!(ed.on_key(Key::Char('a')), RepoOutcome::Add);
        assert_eq!(ed.on_key(Key::Char('q')), RepoOutcome::Close);
    }

    #[test]
    fn refresh_keeps_the_cursor_on_the_same_repo() {
        let mut ed = RepoEditor::new(vec![row("a", false), row("b", false), row("c", false)]);
        ed.cursor = 1;
        ed.refresh(vec![row("c", false), row("b", true)], Some((true, "ok".into())));
        assert_eq!(ed.selected().unwrap().id, "b");
        assert_eq!(ed.message, Some((true, "ok".into())));
        // The selected repo went away: stay in range.
        ed.cursor = 1;
        ed.refresh(vec![row("c", false)], None);
        assert_eq!(ed.cursor, 0);
    }
}
