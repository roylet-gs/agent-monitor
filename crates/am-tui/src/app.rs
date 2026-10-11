//! Dashboard state and key handling. Pure: no I/O, so it is driven directly
//! in tests. Data arrives as a `Snapshot` from the collector (data.rs).

use am_core::model::{Repository, Subagent, WorktreeGroup, WorktreeView};
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
    /// Every tracked repo (also those whose worktrees are all filtered out).
    pub repos: Vec<Repository>,
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

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    None,
    Quit,
    Refresh,
    /// Enter: open in the configured IDE (resuming the last session if set).
    Open(Box<WorktreeView>),
    /// `t`: terminal at the worktree.
    OpenTerminal(Box<WorktreeView>),
    /// `c`: terminal running `claude --resume <latest>`.
    OpenClaude(Box<WorktreeView>),
    /// `o` / `l` / `p`: PR, Linear ticket, Linear project in the browser/app.
    OpenUrl(String),
    /// A setting was saved; theme/collector need to pick it up.
    SettingsChanged,
    /// `e` in settings: open settings.json in $EDITOR.
    EditSettingsFile,
    Create(Box<Repository>, Box<am_core::worktrees::CreateRequest>),
    Delete(Box<WorktreeView>, bool),
    RunScript { repo_id: String, worktree_path: String },
    /// `d` on an "Other sessions" row: stop tracking it.
    RemoveStandalone(String),
    AddRepo(String),
    InstallHooks,
    Suggest { title: String, body: String },
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
    /// Centered message box (e.g. "copied resume command"); any key closes it.
    pub popup: Option<String>,
    /// `w`: debug-log pane at the bottom.
    pub show_log: bool,
    pub settings_editor: Option<crate::settings_ui::SettingsEditor>,
    pub modal: Option<crate::modals::Modal>,
    pub setup: Option<crate::setup::Setup>,
    /// Select this worktree as soon as it shows up (after create).
    pub pending_select: Option<String>,
    /// Worktrees whose status changed since the user last had them selected.
    pub unseen: std::collections::HashSet<String>,
    seen_status: std::collections::HashMap<String, Option<Status>>,
    /// System sounds to play for this update (audio notifications).
    pub sounds: Vec<String>,
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
            popup: None,
            show_log: false,
            settings_editor: None,
            modal: None,
            setup: None,
            pending_select: None,
            unseen: Default::default(),
            seen_status: Default::default(),
            sounds: Vec::new(),
            pending_g: false,
        }
    }

    /// Replace the data, keeping the selection on the same worktree.
    pub fn apply(&mut self, snapshot: Snapshot) {
        let keep = self.pending_select.clone().or_else(|| self.selected_view().map(|v| v.worktree.id.clone()));
        self.snapshot = snapshot;
        self.rebuild_rows();
        if let Some(i) = keep.and_then(|id| self.flat.iter().position(|v| v.worktree.id == id)) {
            self.selected = i;
            if self.pending_select.as_deref() == Some(self.flat[i].worktree.id.as_str()) {
                self.pending_select = None;
            }
        }
        self.selected = self.selected.min(self.flat.len().saturating_sub(1));
        self.sub_selected = self.sub_selected.min(self.visible_subagents().len().saturating_sub(1));
        self.track_status_changes();
        self.loaded = true;
    }

    /// Port of the TS "unseen" logic: a status change on a worktree the user
    /// isn't looking at marks it unseen and (optionally) plays a sound — one
    /// per status type per update, waiting and done only.
    fn track_status_changes(&mut self) {
        let mut changed: Vec<(String, Option<Status>)> = Vec::new();
        for v in &self.flat {
            let now = v.status();
            match self.seen_status.insert(v.worktree.id.clone(), now) {
                Some(prev) if prev != now => changed.push((v.worktree.id.clone(), now)),
                _ => {}
            }
        }
        let first_load = !self.loaded;
        for (id, _) in &changed {
            if !first_load {
                self.unseen.insert(id.clone());
            }
        }
        if self.settings.audio_notifications && !first_load {
            if changed.iter().any(|(_, s)| *s == Some(Status::Waiting)) {
                self.sounds.push(self.settings.audio_waiting_sound.clone());
            }
            if changed.iter().any(|(_, s)| *s == Some(Status::Done)) {
                self.sounds.push(self.settings.audio_done_sound.clone());
            }
        }
        self.mark_selected_seen();
    }

    fn mark_selected_seen(&mut self) {
        if let Some(id) = self.selected_view().map(|v| v.worktree.id.clone()) {
            self.unseen.remove(&id);
        }
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
            if multi_repo || group.repo.id == am_core::model::STANDALONE_REPO_ID {
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
        if self.popup.take().is_some() {
            return Action::None;
        }
        if let Some(setup) = &mut self.setup {
            use crate::setup::SetupEffect;
            if key == Key::CtrlC {
                return Action::Quit;
            }
            return match setup.on_key(key) {
                SetupEffect::None => Action::None,
                SetupEffect::InstallHooks => Action::InstallHooks,
                SetupEffect::AddRepo(p) => Action::AddRepo(p),
                SetupEffect::Finish(kv) => {
                    let mut latest = None;
                    for (k, v) in kv {
                        match crate::settings_ui::persist(k, v) {
                            Ok(s) => latest = Some(s),
                            Err(e) => self.notice = Some(e),
                        }
                    }
                    self.setup = None;
                    match latest {
                        Some(s) => {
                            self.set_settings(s);
                            self.notice = Some("Setup complete — press ? for keys".into());
                            Action::SettingsChanged
                        }
                        None => Action::None,
                    }
                }
            };
        }
        if let Some(modal) = &mut self.modal {
            use crate::modals::Effect;
            return match modal.on_key(key) {
                Effect::None => Action::None,
                Effect::Close => {
                    self.modal = None;
                    Action::None
                }
                Effect::Create(repo, req) => {
                    self.modal = Some(crate::modals::Modal::Busy(format!("Creating {}…", req.branch)));
                    Action::Create(Box::new(repo), Box::new(req))
                }
                Effect::Delete { view, delete_branch } => {
                    self.modal = Some(crate::modals::Modal::Busy(format!("Deleting {}…", view.worktree.display_name())));
                    Action::Delete(view, delete_branch)
                }
                Effect::Suggest { title, body } => {
                    self.modal = Some(crate::modals::Modal::Busy("Filing feature request…".into()));
                    Action::Suggest { title, body }
                }
                Effect::AddRepo(path) => {
                    self.modal = Some(crate::modals::Modal::Busy(format!("Adding {path}…")));
                    Action::AddRepo(path)
                }
                Effect::RunScript { repo_id, worktree_path } => {
                    self.modal = None;
                    Action::RunScript { repo_id, worktree_path }
                }
            };
        }
        if let Some(editor) = &mut self.settings_editor {
            use crate::settings_ui::Outcome;
            return match editor.on_key(key) {
                Outcome::None => Action::None,
                Outcome::Close => {
                    self.settings_editor = None;
                    Action::None
                }
                Outcome::Changed(s) => {
                    self.set_settings(*s);
                    Action::SettingsChanged
                }
                Outcome::EditFile => Action::EditSettingsFile,
            };
        }
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
            Key::Enter => return self.with_selected(Action::Open),
            Key::Char('t') => return self.with_selected(Action::OpenTerminal),
            Key::Char('c') => return self.with_selected(Action::OpenClaude),
            Key::Char('o') => return self.url_action(|v, _| {
                let pr = v.pr.as_ref()?;
                // Same rule as the TS app: a merged PR jumps to its last active check.
                Some(if pr.state == "MERGED" { pr.active_check_url.clone().unwrap_or(pr.url.clone()) } else { pr.url.clone() })
            }, "No PR for this branch"),
            Key::Char('l') => return self.url_action(|v, s| v.linear.as_ref().map(|l| linear_url(&l.url, s)), "No Linear ticket"),
            Key::Char('p') => return self.url_action(
                |v, s| v.linear.as_ref()?.project.as_ref()?.url.as_ref().map(|u| linear_url(u, s)),
                "No Linear project",
            ),
            Key::Char('w') => self.show_log = !self.show_log,
            Key::Char('n') => {
                if self.snapshot.repos.is_empty() {
                    self.modal = Some(crate::modals::Modal::AddRepo { path: "~/".into(), error: None });
                } else {
                    let repo = self.selected_view().map(|v| v.worktree.repo_id.clone());
                    self.modal = Some(crate::modals::Modal::new_worktree(
                        self.snapshot.repos.clone(),
                        repo.as_deref(),
                        &self.settings.default_branch_prefix,
                        &self.settings.default_base_branch,
                    ));
                }
            }
            Key::Char('d') => {
                if let Some(v) = self.selected_view().cloned() {
                    if v.worktree.is_standalone() {
                        return Action::RemoveStandalone(v.worktree.path);
                    }
                    if v.worktree.is_main && (v.worktree.branch == "main" || v.worktree.branch == "master") {
                        self.notice = Some("The main checkout can't be deleted".into());
                    } else {
                        self.modal = Some(crate::modals::Modal::delete(v));
                    }
                }
            }
            Key::Char('f') => {
                self.modal = Some(crate::modals::Modal::Suggest { title: String::new(), body: String::new(), on_body: false, error: None })
            }
            Key::Char('A') => self.modal = Some(crate::modals::Modal::AddRepo { path: "~/".into(), error: None }),
            Key::Char('s') => self.settings_editor = Some(crate::settings_ui::SettingsEditor::new(&self.settings)),
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

    /// Swap in new settings and re-derive rows (header style depends on sort).
    pub fn set_settings(&mut self, settings: Settings) {
        self.settings = settings;
        if let Some(e) = &mut self.settings_editor {
            let cursor = e.cursor;
            *e = crate::settings_ui::SettingsEditor::new(&self.settings);
            e.cursor = cursor;
        }
        let snap = self.snapshot.clone();
        self.apply(snap);
    }

    fn url_action(&mut self, pick: impl Fn(&WorktreeView, &Settings) -> Option<String>, missing: &str) -> Action {
        match self.selected_view().and_then(|v| pick(v, &self.settings)) {
            Some(url) => Action::OpenUrl(url),
            None => {
                self.notice = Some(missing.to_string());
                Action::None
            }
        }
    }

    fn with_selected(&self, f: fn(Box<WorktreeView>) -> Action) -> Action {
        self.selected_view().map(|v| f(Box::new(v.clone()))).unwrap_or(Action::None)
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
            self.mark_selected_seen();
        }
    }
}

fn linear_url(url: &str, s: &Settings) -> String {
    if s.linear_use_desktop_app {
        url.replacen("https://linear.app/", "linear://", 1)
    } else {
        url.to_string()
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
    Left,
    Right,
    Backspace,
    Tab,
    BackTab,
    Enter,
    Esc,
    CtrlC,
    CtrlD,
    CtrlU,
}
