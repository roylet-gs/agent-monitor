//! Dashboard state and key handling. Pure: no I/O, so it is driven directly
//! in tests. Data arrives as a `Snapshot` from the collector (data.rs).

use am_core::model::{Subagent, WorktreeGroup, WorktreeView};
use am_core::settings::{SortKey, Settings};
use am_core::status::Status;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Health {
    pub github_failing: bool,
    pub linear_failing: bool,
}

/// One consistent view of everything on screen.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    pub groups: Vec<WorktreeGroup>,
    /// (worktree_id, subagent) — running first, then recently finished.
    pub subagents: Vec<(Option<String>, Subagent)>,
    pub health: Health,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Worktrees,
    Subagents,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Selected,
    All,
}

/// A visual row of the worktree list. Headers never take a selection index:
/// `flat` order always equals the visual order of `Row::Worktree` entries.
#[derive(Debug, Clone, PartialEq)]
pub enum Row {
    Repo(String),
    Project(String),
    Ticket(String),
    Worktree(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    None,
    Quit,
    Refresh,
}

pub struct App {
    pub settings: Settings,
    pub snapshot: Snapshot,
    pub flat: Vec<WorktreeView>,
    pub rows: Vec<Row>,
    pub selected: usize,
    pub focus: Pane,
    pub scope: Scope,
    pub sub_selected: usize,
    pub show_help: bool,
    pub loaded: bool,
    pub notice: Option<String>,
    pending_g: bool,
}

impl App {
    pub fn new(settings: Settings) -> App {
        App {
            settings,
            snapshot: Snapshot::default(),
            flat: Vec::new(),
            rows: Vec::new(),
            selected: 0,
            focus: Pane::Worktrees,
            scope: Scope::Selected,
            sub_selected: 0,
            show_help: false,
            loaded: false,
            notice: None,
            pending_g: false,
        }
    }

    /// Replace the data, keeping the selection on the same worktree.
    pub fn apply(&mut self, snapshot: Snapshot) {
        let keep = self.selected_view().map(|v| v.worktree.id.clone());
        self.snapshot = snapshot;
        self.rebuild_rows();
        if let Some(i) = keep.and_then(|id| self.flat.iter().position(|v| v.worktree.id == id)) {
            self.selected = i;
        }
        self.selected = self.selected.min(self.flat.len().saturating_sub(1));
        self.sub_selected = self.sub_selected.min(self.visible_subagents().len().saturating_sub(1));
        self.loaded = true;
    }

    fn rebuild_rows(&mut self) {
        let by_project = am_core::grouping::top_sort_key(&self.settings) == Some(SortKey::LinearProject);
        let multi_repo = self.snapshot.groups.len() > 1;
        self.flat.clear();
        self.rows.clear();
        for group in &self.snapshot.groups {
            if group.worktrees.is_empty() {
                continue;
            }
            if multi_repo {
                self.rows.push(Row::Repo(group.repo.name.clone()));
            }
            let mut last_project: Option<Option<String>> = None;
            let wts = &group.worktrees;
            for (i, v) in wts.iter().enumerate() {
                if by_project {
                    let project = v.linear.as_ref().and_then(|l| l.project.as_ref()).map(|p| p.name.clone());
                    if last_project.as_ref() != Some(&project) {
                        self.rows.push(Row::Project(project.clone().unwrap_or_else(|| "No project".into())));
                        last_project = Some(project);
                    }
                }
                let ticket = ticket_of(v);
                let starts_cluster = ticket.is_some()
                    && (i == 0 || ticket_of(&wts[i - 1]) != ticket)
                    && wts.get(i + 1).is_some_and(|n| ticket_of(n) == ticket);
                if starts_cluster {
                    let l = v.linear.as_ref().unwrap();
                    self.rows.push(Row::Ticket(format!("{} {}", l.identifier, l.title)));
                }
                self.rows.push(Row::Worktree(self.flat.len()));
                self.flat.push(v.clone());
            }
        }
    }

    pub fn selected_view(&self) -> Option<&WorktreeView> {
        self.flat.get(self.selected)
    }

    pub fn visible_subagents(&self) -> Vec<&(Option<String>, Subagent)> {
        match self.scope {
            Scope::All => self.snapshot.subagents.iter().collect(),
            Scope::Selected => {
                let Some(id) = self.selected_view().map(|v| &v.worktree.id) else { return Vec::new() };
                self.snapshot.subagents.iter().filter(|(w, _)| w.as_ref() == Some(id)).collect()
            }
        }
    }

    pub fn running_subagents_for(&self, worktree_id: &str) -> usize {
        self.snapshot.subagents.iter().filter(|(w, s)| s.running && w.as_deref() == Some(worktree_id)).count()
    }

    pub fn status_counts(&self) -> Vec<(Status, usize)> {
        [Status::Executing, Status::Planning, Status::Waiting, Status::Delegating, Status::Done]
            .into_iter()
            .map(|s| (s, self.flat.iter().filter(|v| v.status() == Some(s)).count()))
            .filter(|(_, n)| *n > 0)
            .collect()
    }

    pub fn on_key(&mut self, key: Key) -> Action {
        if self.show_help {
            if matches!(key, Key::Char('?') | Key::Esc | Key::Char('q')) {
                self.show_help = false;
            }
            return Action::None;
        }
        let g = std::mem::take(&mut self.pending_g);
        match key {
            Key::Char('q') | Key::CtrlC => return Action::Quit,
            Key::Esc if self.focus == Pane::Subagents => self.focus = Pane::Worktrees,
            Key::Esc => return Action::Quit,
            Key::Char('?') => self.show_help = true,
            Key::Char('r') => {
                self.notice = Some("Refreshing…".into());
                return Action::Refresh;
            }
            Key::Tab | Key::BackTab => self.toggle_focus(),
            Key::Char('1') => self.focus = Pane::Worktrees,
            Key::Char('2') => self.focus = Pane::Subagents,
            Key::Char('a') => {
                self.scope = match self.scope {
                    Scope::Selected => Scope::All,
                    Scope::All => Scope::Selected,
                };
                self.sub_selected = 0;
            }
            Key::Down | Key::Char('j') => self.move_by(1),
            Key::Up | Key::Char('k') => self.move_by(-1),
            Key::PageDown | Key::CtrlD => self.move_by(10),
            Key::PageUp | Key::CtrlU => self.move_by(-10),
            Key::Char('G') | Key::End => self.move_by(isize::MAX / 2),
            Key::Home => self.move_by(isize::MIN / 2),
            Key::Char('g') if g => self.move_by(isize::MIN / 2),
            Key::Char('g') => self.pending_g = true,
            _ => {}
        }
        Action::None
    }

    fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            Pane::Worktrees => Pane::Subagents,
            Pane::Subagents => Pane::Worktrees,
        };
    }

    fn move_by(&mut self, delta: isize) {
        let sub_len = self.visible_subagents().len();
        let (cur, len) = match self.focus {
            Pane::Worktrees => (&mut self.selected, self.flat.len()),
            Pane::Subagents => (&mut self.sub_selected, sub_len),
        };
        if len == 0 {
            return;
        }
        let next = (*cur as isize).saturating_add(delta).clamp(0, len as isize - 1) as usize;
        *cur = next;
        if self.focus == Pane::Worktrees {
            self.sub_selected = 0;
        }
    }
}

fn ticket_of(v: &WorktreeView) -> Option<&str> {
    v.linear.as_ref().map(|l| l.identifier.as_str())
}

/// Backend-independent key events (mapped from crossterm in main.rs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
    Tab,
    BackTab,
    Enter,
    Esc,
    CtrlC,
    CtrlD,
    CtrlU,
}
