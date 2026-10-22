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
    /// Stacked branches: child worktree id → parent worktree id.
    pub stack: std::collections::HashMap<String, String>,
    /// Usage today (`showDailyTokens`/`dailyTokensScope`), shown per
    /// `dailyTokensMode`; `None` when the setting is off.
    pub today_tokens: Option<am_core::usage::Tally>,
}

/// A visual row of the worktree list. Headers never take a selection index:
/// `flat` order always equals the visual order of `Row::Worktree` entries.
#[derive(Debug, Clone, PartialEq)]
pub enum Row {
    /// Blank line separating repo sections.
    Spacer,
    Repo(String),
    Project(String),
    Ticket(String),
    Worktree(usize),
    /// Placeholder for a worktree being created (jobs-pane job id). Like
    /// headers, it takes no selection index.
    Pending(u64),
    /// One of several live agent sessions in a worktree, listed under it:
    /// (`flat` index, index into its `sessions`). No selection index either;
    /// a click selects its worktree.
    Session(usize, usize),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    None,
    Quit,
    Refresh,
    /// Enter: open in the configured IDE (resuming the last session if set).
    Open(Box<WorktreeView>),
    /// `o` / `l` / `p`: PR, Linear ticket, Linear project in the browser/app.
    OpenUrl(String),
    /// A setting was saved; theme/collector need to pick it up.
    SettingsChanged,
    /// `e` in settings: open settings.json in the `ide` (editor or custom command), or
    /// `$EDITOR` in this terminal when `ide` is `terminal`.
    EditSettingsFile,
    /// Open debug.log in the `ide`, or reveal the log folder once it has rotated.
    ViewLogs,
    /// Job id, repo and request.
    Create(u64, Box<Repository>, Box<am_core::worktrees::CreateRequest>),
    /// Job id, worktree, and whether to delete the branch too.
    Delete(u64, Box<WorktreeView>, bool),
    RunScript { job: u64, repo_id: String, worktree_path: String },
    /// `d` on an "Other sessions" row: stop tracking it.
    RemoveStandalone(String),
    /// `T`: put a worktree in the tmp space (or take it out): id, new value.
    SetTmpSpace(String, bool),
    AddRepo(String),
    /// Settings → Repositories… `s`: open the repo's startup script.
    EditScript(String),
    /// A repository was removed in Settings: refresh the dashboard.
    ReposChanged,
    InstallHooks,
    /// Settings → Run setup again: open the wizard (main.rs reads the repos).
    RunSetup,
    /// Settings → Reset settings, confirmed: reset settings.json, then run setup.
    ResetSettings,
    Suggest { title: String, body: String },
}

pub struct App {
    pub settings: Settings,
    pub snapshot: Snapshot,
    pub flat: Vec<WorktreeView>,
    /// Tree-line prefix per `flat` entry ("" for top level, "└─ ", "│  ├─ "…).
    pub tree: Vec<String>,
    pub rows: Vec<Row>,
    pub selected: usize,
    pub show_help: bool,
    pub loaded: bool,
    pub notice: Option<String>,
    /// Newer release found by the update check: top right of the pane, and
    /// a banner with the upgrade command at the top of Settings.
    pub update_available: Option<am_core::update_check::Update>,
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
    /// Worktrees being deleted in the background → whether the job has
    /// finished. Their rows are greyed out and take no actions; a finished
    /// entry is dropped on the next snapshot, so the row doesn't flash back
    /// to normal before the refresh removes it.
    pub deleting: std::collections::HashMap<String, bool>,
    /// Background create/delete jobs (right-hand jobs pane).
    pub jobs: crate::jobs::Jobs,
    /// Dashboard keys (`keyBindings`), rebuilt whenever settings change.
    pub keymap: crate::keymap::Keymap,
    /// Last mouse position (cell), for the hover highlight on list rows.
    /// Hit-tested at draw time, so it follows scrolling and refreshes.
    pub hover: Option<(u16, u16)>,
    /// What a click does where, recorded by the last `ui::draw` for the
    /// layer on top (so clicks always match what's on screen).
    pub hits: std::cell::RefCell<Vec<crate::ui::Hit>>,
}

impl App {
    pub fn new(settings: Settings) -> App {
        App {
            keymap: crate::keymap::Keymap::from_settings(&settings),
            settings,
            snapshot: Snapshot::default(),
            flat: Vec::new(),
            tree: Vec::new(),
            rows: Vec::new(),
            selected: 0,
            show_help: false,
            loaded: false,
            notice: None,
            update_available: None,
            popup: None,
            show_log: false,
            settings_editor: None,
            modal: None,
            setup: None,
            pending_select: None,
            unseen: Default::default(),
            seen_status: Default::default(),
            sounds: Vec::new(),
            deleting: Default::default(),
            jobs: Default::default(),
            hover: None,
            hits: Default::default(),
        }
    }

    /// Replace the data, keeping the selection on the same worktree.
    pub fn apply(&mut self, snapshot: Snapshot) {
        let keep = self.pending_select.clone().or_else(|| self.selected_view().map(|v| v.worktree.id.clone()));
        self.snapshot = snapshot;
        self.deleting.retain(|_, finished| !*finished);
        self.rebuild_rows();
        if let Some(i) = keep.and_then(|id| self.flat.iter().position(|v| v.worktree.id == id)) {
            self.selected = i;
            if self.pending_select.as_deref() == Some(self.flat[i].worktree.id.as_str()) {
                self.pending_select = None;
            }
        }
        self.selected = self.selected.min(self.flat.len().saturating_sub(1));
        self.track_status_changes();
        if let Some(e) = &mut self.settings_editor {
            e.repo_count = self.snapshot.repos.len();
        }
        self.loaded = true;
    }

    /// The add-repository folder browser, opened next to the newest repo.
    pub fn add_repo_modal(&self) -> crate::modals::Modal {
        let paths: Vec<String> = self.snapshot.repos.iter().map(|r| r.path.clone()).collect();
        let dir = crate::folder_browser::start_dir(&paths);
        crate::modals::Modal::AddRepo(crate::folder_browser::FolderBrowser::open(dir, paths))
    }

    fn open_settings(&mut self) {
        let mut e = crate::settings_ui::SettingsEditor::new(&self.settings);
        e.repo_count = self.snapshot.repos.len();
        self.settings_editor = Some(e);
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
            // Each listed session counts too: one finishing while another
            // keeps the headline busy still marks the worktree and plays.
            if v.sessions.len() > 1 {
                for s in &v.sessions {
                    let key = format!("{}\u{1f}{}", v.worktree.id, s.session_id);
                    match self.seen_status.insert(key, Some(s.status)) {
                        Some(prev) if prev != Some(s.status) => changed.push((v.worktree.id.clone(), Some(s.status))),
                        _ => {}
                    }
                }
            }
        }
        let first_load = !self.loaded;
        for (id, _) in &changed {
            if !first_load {
                self.unseen.insert(id.clone());
            }
        }
        if !first_load && !changed.is_empty() {
            am_core::debug!(
                "audio",
                "status changes: {} (sounds {})",
                changed.iter().map(|(id, s)| format!("{id}→{}", s.map_or("none", Status::as_str))).collect::<Vec<_>>().join(", "),
                if self.settings.audio_notifications { "on" } else { "off" }
            );
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

    /// Re-derive the visual rows (placeholder rows follow the jobs). `flat`
    /// keeps its order, so the selection index stays valid.
    pub fn rebuild_rows(&mut self) {
        let by_project = am_core::grouping::top_sort_key(&self.settings) == Some(SortKey::LinearProject);
        let multi_repo = self.snapshot.groups.len() > 1;
        self.flat.clear();
        self.rows.clear();
        self.tree.clear();
        // Creates whose worktree isn't in the snapshot yet: (job, group id, group name).
        // Tmp-space creates wait at the end of the tmp space.
        let listed = |id: &str| self.snapshot.groups.iter().any(|g| g.worktrees.iter().any(|v| v.worktree.id == id));
        let mut pending: Vec<(u64, String, String)> = self
            .jobs
            .list
            .iter()
            .filter_map(|j| {
                let (repo, req, wt) = j.pending_create()?;
                let (id, name) = match req.tmp_space {
                    true => (am_core::model::TMP_SPACE_GROUP_ID.to_string(), "Tmp space".to_string()),
                    false => (repo.id.clone(), repo.name.clone()),
                };
                (!wt.is_some_and(listed)).then_some((j.id, id, name))
            })
            .collect();
        for group in &self.snapshot.groups {
            if group.worktrees.is_empty() {
                continue;
            }
            if multi_repo || group.repo.id == am_core::model::STANDALONE_REPO_ID || group.repo.id == am_core::model::TMP_SPACE_GROUP_ID {
                if !self.rows.is_empty() {
                    self.rows.push(Row::Spacer);
                }
                self.rows.push(Row::Repo(group.repo.name.clone()));
            }
            let ordered = stack_order(&group.worktrees, &self.snapshot.stack);
            // Headers (project / ticket cluster) only consider top-level rows,
            // so a stack is never split by a header.
            let tops: Vec<usize> = ordered.iter().filter(|(_, prefix)| prefix.is_empty()).map(|(i, _)| *i).collect();
            let mut last_project: Option<Option<String>> = None;
            let mut top_pos: usize = 0;
            // Set after the last top-level row of a ticket cluster: whatever
            // follows the cluster (and its stacked children) gets a blank line.
            let mut cluster_ended = false;
            for (i, prefix) in ordered {
                let v = &group.worktrees[i];
                if prefix.is_empty() {
                    if std::mem::take(&mut cluster_ended) && !matches!(self.rows.last(), Some(Row::Spacer)) {
                        self.rows.push(Row::Spacer);
                    }
                    if by_project {
                        let project = v.linear.as_ref().and_then(|l| l.project.as_ref()).map(|p| p.name.clone());
                        if last_project.as_ref() != Some(&project) {
                            // Breathing room between project sections (not right under the repo header).
                            if !matches!(self.rows.last(), None | Some(Row::Repo(_)) | Some(Row::Spacer)) {
                                self.rows.push(Row::Spacer);
                            }
                            // Rows without a project get the spacer but no header.
                            if let Some(name) = &project {
                                self.rows.push(Row::Project(name.clone()));
                            }
                            last_project = Some(project);
                        }
                    }
                    let ticket = ticket_of(v);
                    let prev = top_pos.checked_sub(1).map(|p| ticket_of(&group.worktrees[tops[p]]));
                    let next = tops.get(top_pos + 1).map(|&n| ticket_of(&group.worktrees[n]));
                    if ticket.is_some() && prev != Some(ticket) && next == Some(ticket) {
                        // A blank line above the cluster too, unless it opens a section.
                        if !matches!(self.rows.last(), None | Some(Row::Repo(_)) | Some(Row::Project(_)) | Some(Row::Spacer)) {
                            self.rows.push(Row::Spacer);
                        }
                        let l = v.linear.as_ref().unwrap();
                        self.rows.push(Row::Ticket(format!("{} {}", l.identifier, l.title)));
                    }
                    cluster_ended = ticket.is_some() && prev == Some(ticket) && next != Some(ticket);
                    top_pos += 1;
                }
                let i = self.flat.len();
                self.rows.push(Row::Worktree(i));
                if v.sessions.len() > 1 {
                    self.rows.extend((0..v.sessions.len()).map(|k| Row::Session(i, k)));
                }
                self.flat.push(v.clone());
                self.tree.push(prefix);
            }
            pending.retain(|(job, repo_id, _)| {
                let here = *repo_id == group.repo.id;
                if here {
                    if std::mem::take(&mut cluster_ended) {
                        self.rows.push(Row::Spacer);
                    }
                    self.rows.push(Row::Pending(*job));
                }
                !here
            });
        }
        // Creates in a repo with no visible rows yet get their own section.
        pending.sort_by(|a, b| a.1.cmp(&b.1));
        let mut last_repo: Option<String> = None;
        for (job, repo_id, name) in pending {
            if last_repo.as_ref() != Some(&repo_id) {
                if !self.rows.is_empty() {
                    self.rows.push(Row::Spacer);
                }
                self.rows.push(Row::Repo(name));
                last_repo = Some(repo_id);
            }
            self.rows.push(Row::Pending(job));
        }
    }

    /// A background delete finished. On failure the row is usable again
    /// right away; on success it stays greyed until the next snapshot.
    pub fn finish_delete(&mut self, id: &str, ok: bool) {
        if ok {
            if let Some(finished) = self.deleting.get_mut(id) {
                *finished = true;
            }
        } else {
            self.deleting.remove(id);
        }
    }

    /// Whether a key-bar action applies to the selected worktree (the key
    /// bar dims the ones that don't). Keys that aren't per-worktree are
    /// always available.
    pub fn key_available(&self, cmd: crate::keymap::Command) -> bool {
        use crate::keymap::Command as C;
        if !cmd.is_row_action() {
            return true;
        }
        let Some(v) = self.selected_view() else { return false };
        if self.is_deleting(&v.worktree.id) {
            return false;
        }
        let w = &v.worktree;
        match cmd {
            C::OpenPr => v.pr.is_some(),
            C::OpenLinear => v.linear.is_some(),
            C::OpenProject => v.linear.as_ref().and_then(|l| l.project.as_ref()).is_some_and(|p| p.url.is_some()),
            C::Delete => w.is_standalone() || !(w.is_main && (w.branch == "main" || w.branch == "master")),
            C::ToggleTmpSpace => !w.is_standalone() && !w.is_main,
            _ => true,
        }
    }

    pub fn is_deleting(&self, id: &str) -> bool {
        self.deleting.contains_key(id)
    }

    pub fn selected_view(&self) -> Option<&WorktreeView> {
        self.flat.get(self.selected)
    }


    pub fn running_subagents_for(&self, worktree_id: &str) -> usize {
        self.snapshot.subagents.iter().filter(|(w, s)| s.running && w.as_deref() == Some(worktree_id)).count()
    }

    /// Running subagents of one session in a worktree.
    pub fn running_subagents_in(&self, worktree_id: &str, session_id: &str) -> usize {
        self.snapshot
            .subagents
            .iter()
            .filter(|(w, s)| s.running && w.as_deref() == Some(worktree_id) && s.session_id == session_id)
            .count()
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
                SetupEffect::Cancel => {
                    self.setup = None;
                    self.notice = Some("Setup closed — settings unchanged".into());
                    Action::None
                }
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
                    self.modal = None;
                    let job = self.jobs.start_create(repo.clone(), req.clone());
                    self.rebuild_rows();
                    Action::Create(job, Box::new(repo), Box::new(req))
                }
                Effect::Delete { view, delete_branch } => {
                    self.modal = None;
                    let id = view.worktree.id.clone();
                    self.deleting.insert(id.clone(), false);
                    let job = self.jobs.start_delete(id, view.worktree.display_name());
                    Action::Delete(job, view, delete_branch)
                }
                Effect::Suggest { title, body } => {
                    self.modal = Some(crate::modals::Modal::busy("Filing feature request…".into()));
                    Action::Suggest { title, body }
                }
                // The browser stays open (showing "Adding…") until the result comes back.
                Effect::AddRepo(path) => Action::AddRepo(path),
                Effect::ResetSettings => {
                    self.modal = None;
                    Action::ResetSettings
                }
            };
        }
        if let Some(editor) = &mut self.settings_editor {
            let outcome = editor.on_key(key);
            return self.editor_outcome(outcome);
        }
        if self.show_help {
            if matches!(key, Key::Char('?') | Key::Esc | Key::Char('q')) {
                self.show_help = false;
            }
            return Action::None;
        }
        if self.jobs.focused {
            use crate::jobs::JobEffect;
            let effect = self.jobs.on_key(key);
            self.rebuild_rows();
            return match effect {
                JobEffect::None => Action::None,
                JobEffect::Create(job, repo, req) => Action::Create(job, repo, req),
                JobEffect::RunScript { job, repo_id, worktree_path } => Action::RunScript { job, repo_id, worktree_path },
            };
        }
        self.dashboard_key(key)
    }

    fn editor_outcome(&mut self, outcome: crate::settings_ui::Outcome) -> Action {
        use crate::settings_ui::Outcome;
        match outcome {
            Outcome::None => Action::None,
            Outcome::Close => {
                self.settings_editor = None;
                Action::None
            }
            Outcome::Changed(s) => {
                // Picking a sound plays it, so the choice can be heard.
                if let Some(key) = self.settings_editor.as_ref().map(|e| e.field().key) {
                    let pick = match key {
                        "audioWaitingSound" => Some(s.audio_waiting_sound.clone()),
                        "audioDoneSound" => Some(s.audio_done_sound.clone()),
                        _ => None,
                    };
                    self.sounds.extend(pick);
                }
                self.set_settings(*s);
                Action::SettingsChanged
            }
            Outcome::EditFile => Action::EditSettingsFile,
            Outcome::AddRepo => {
                self.modal = Some(self.add_repo_modal());
                Action::None
            }
            Outcome::EditScript(id) => Action::EditScript(id),
            Outcome::ReposChanged => Action::ReposChanged,
            Outcome::RunSetup => Action::RunSetup,
            Outcome::ConfirmReset => {
                self.modal = Some(crate::modals::Modal::ConfirmReset);
                Action::None
            }
        }
    }

    fn dashboard_key(&mut self, key: Key) -> Action {
        if key == Key::Tab && self.jobs.visible() {
            self.jobs.focus();
            return Action::None;
        }
        match self.keymap.command(key) {
            Some(cmd) => self.run_command(cmd),
            None => Action::None,
        }
    }

    /// Run a dashboard command (from its key, a key-bar click or a double-click).
    pub fn run_command(&mut self, cmd: crate::keymap::Command) -> Action {
        use crate::keymap::Command as C;
        if cmd.is_row_action() && self.selected_view().is_some_and(|v| self.is_deleting(&v.worktree.id)) {
            self.notice = Some("Deleting in the background…".into());
            return Action::None;
        }
        match cmd {
            C::Quit => return Action::Quit,
            C::Help => self.show_help = true,
            C::Refresh => {
                self.notice = Some("Refreshing…".into());
                return Action::Refresh;
            }
            C::Open => return self.with_selected(Action::Open),
            C::OpenPr => return self.url_action(|v, _| {
                let pr = v.pr.as_ref()?;
                // Same rule as the TS app: a merged PR jumps to its last active check.
                Some(if pr.state == "MERGED" { pr.active_check_url.clone().unwrap_or(pr.url.clone()) } else { pr.url.clone() })
            }, "No PR for this branch"),
            C::OpenLinear => return self.url_action(|v, s| v.linear.as_ref().map(|l| linear_url(&l.url, s)), "No Linear ticket"),
            C::OpenProject => return self.url_action(
                |v, s| v.linear.as_ref()?.project.as_ref()?.url.as_ref().map(|u| linear_url(u, s)),
                "No Linear project",
            ),
            C::ToggleLog => self.show_log = !self.show_log,
            C::ViewLogs => return Action::ViewLogs,
            C::NewWorktree => {
                if self.snapshot.repos.is_empty() {
                    self.modal = Some(self.add_repo_modal());
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
            C::Delete => {
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
            C::ToggleTmpSpace => {
                if let Some(w) = self.selected_view().map(|v| v.worktree.clone()) {
                    if w.is_standalone() || w.is_main {
                        self.notice = Some("Only linked worktrees can go in the tmp space".into());
                    } else {
                        return Action::SetTmpSpace(w.id, !w.tmp_space);
                    }
                }
            }
            C::Feedback => {
                self.modal = Some(crate::modals::Modal::Suggest { title: String::new(), body: String::new(), on_body: false, error: None })
            }
            C::AddRepo => self.modal = Some(self.add_repo_modal()),
            C::Settings => self.open_settings(),
            C::MoveDown => self.move_by(1),
            C::MoveUp => self.move_by(-1),
            C::PageDown => self.move_by(10),
            C::PageUp => self.move_by(-10),
            C::Bottom => self.move_by(isize::MAX / 2),
            C::Top => self.move_by(isize::MIN / 2),
        }
        Action::None
    }

    /// Mouse input, with the same overlay priority as `on_key`. Clicks are
    /// hit-tested against the layout `ui` draws for a terminal of size `area`,
    /// then mostly turned into the equivalent key.
    /// Mouse input, hit-tested against what the last frame drew
    /// (`App::hits`): the wheel acts like ↑/↓ in whatever is on top, and a
    /// click does what its spot on screen says.
    pub fn on_mouse(&mut self, m: Mouse) -> Action {
        use crate::ui::Target;
        let (x, y, double) = match m {
            Mouse::Move { x, y } => {
                self.hover = Some((x, y));
                return Action::None;
            }
            Mouse::Scroll(d) => {
                // Help closes on any key; the reset prompt cancels on one;
                // a key-capture would take the arrow as the new binding.
                let capturing = self.settings_editor.as_ref().and_then(|e| e.keys.as_ref()).is_some_and(|k| k.capturing);
                if self.popup.is_some()
                    || (self.show_help && self.settings_editor.is_none() && self.setup.is_none() && self.modal.is_none())
                    || matches!(self.modal, Some(crate::modals::Modal::ConfirmReset))
                    || capturing
                {
                    return Action::None;
                }
                return self.on_key(if d > 0 { Key::Down } else { Key::Up });
            }
            Mouse::Click { x, y, double } => (x, y, double),
        };
        if self.popup.take().is_some() {
            return Action::None;
        }
        let hit = crate::ui::hit_at(&self.hits.borrow(), x, y);
        let Some(hit) = hit else {
            // Help is a read-only overlay: a click anywhere closes it.
            if self.show_help && self.settings_editor.is_none() && self.setup.is_none() && self.modal.is_none() {
                self.show_help = false;
            }
            return Action::None;
        };
        match hit.target {
            Target::Key(k) => self.on_key(k),
            Target::Command(c) => self.run_command(c),
            Target::JobsPane => {
                self.jobs.focus();
                Action::None
            }
            Target::JobKey(job, k) => {
                self.jobs.focus();
                self.jobs.cursor = job;
                self.on_key(k)
            }
            Target::Worktree(i) => {
                self.jobs.focused = false;
                self.selected = i;
                self.mark_selected_seen();
                if double { self.run_command(crate::keymap::Command::Open) } else { Action::None }
            }
            Target::Item(list, i) => self.pick(list, i),
        }
    }

    /// A click on row `i` of `list`: move the list's cursor there, or, when
    /// it's already there, act on the row like its main key does.
    fn pick(&mut self, list: crate::ui::List, i: usize) -> Action {
        use crate::modals::{FormField, Modal};
        use crate::theme_builder::Mode;
        use crate::ui::List as L;
        // Move the cursor; `Some(key)` = it was already there, press `key`.
        let act: Option<Key> = match list {
            L::Settings => {
                let Some(editor) = &mut self.settings_editor else { return Action::None };
                let outcome = editor.click(i);
                return self.editor_outcome(outcome);
            }
            L::Sort | L::Plugins | L::Repos | L::Keys | L::ThemeTokens | L::Palette => {
                let Some(e) = &mut self.settings_editor else { return Action::None };
                let at = |cursor: &mut usize| std::mem::replace(cursor, i) == i;
                match list {
                    L::Sort => e.sort.as_mut().and_then(|s| at(&mut s.cursor).then_some(Key::Char(' '))),
                    L::Plugins => e.plugins.as_mut().and_then(|p| at(&mut p.cursor).then_some(Key::Char(' '))),
                    L::Repos => e.repos.as_mut().and_then(|r| {
                        r.confirm_remove = None;
                        r.message = None;
                        at(&mut r.cursor).then_some(Key::Enter)
                    }),
                    L::Keys => e.keys.as_mut().and_then(|k| {
                        k.message = None;
                        at(&mut k.cursor).then_some(Key::Enter)
                    }),
                    L::ThemeTokens => e.builder.as_mut().and_then(|b| {
                        // From the palette, a token click goes back to the list.
                        if matches!(b.mode, Mode::Picker { .. }) {
                            b.mode = Mode::Tokens;
                        }
                        at(&mut b.cursor).then_some(Key::Enter)
                    }),
                    _ => e.builder.as_mut().and_then(|b| match &mut b.mode {
                        Mode::Picker { cursor } => at(cursor).then_some(Key::Enter),
                        _ => None,
                    }),
                }
            }
            L::Browser => match &mut self.modal {
                Some(Modal::AddRepo(b)) if b.adding.is_none() => {
                    b.error = None;
                    (std::mem::replace(&mut b.cursor, i) == i).then_some(Key::Enter)
                }
                _ => None,
            },
            L::NewForm => match &mut self.modal {
                Some(Modal::New(f)) => {
                    let which = [FormField::Repo, FormField::Branch, FormField::Name, FormField::Base, FormField::TmpSpace][i.min(4)];
                    // The repo row cycles on a second click; the tmp space switch flips on the first.
                    let again = std::mem::replace(&mut f.field, which) == which;
                    ((again && which == FormField::Repo) || which == FormField::TmpSpace).then_some(Key::Right)
                }
                _ => None,
            },
            L::Suggest => {
                if let Some(Modal::Suggest { on_body, .. }) = &mut self.modal {
                    *on_body = i == 1;
                }
                None
            }
            L::SetupIde | L::SetupIntegrations | L::SetupContext => {
                let Some(s) = &mut self.setup else { return Action::None };
                s.ide_error = None;
                s.ctx_error = None;
                match list {
                    L::SetupIde => (std::mem::replace(&mut s.ide, i) == i).then_some(Key::Enter),
                    L::SetupContext => (std::mem::replace(&mut s.ctx_choice, i) == i).then_some(Key::Enter),
                    // The GitHub checkbox toggles on a second click.
                    _ => (std::mem::replace(&mut s.integration_field, i) == i && i == 0).then_some(Key::Char(' ')),
                }
            }
            L::Jobs => {
                self.jobs.focus();
                self.jobs.cursor = i;
                None
            }
        };
        match act {
            Some(k) => self.on_key(k),
            None => Action::None,
        }
    }

    /// Swap in new settings and re-derive rows (header style depends on sort).
    pub fn set_settings(&mut self, settings: Settings) {
        self.keymap = crate::keymap::Keymap::from_settings(&settings);
        self.settings = settings;
        if let Some(e) = &mut self.settings_editor {
            let cursor = e.cursor;
            // The agent-plugins and key-bindings pages report changes while they stay open.
            let plugins = e.plugins.take();
            let keys = e.keys.take();
            let repos = e.repos.take();
            let repo_count = e.repo_count;
            *e = crate::settings_ui::SettingsEditor::new(&self.settings);
            e.cursor = cursor;
            e.plugins = plugins;
            e.keys = keys;
            e.repos = repos;
            e.repo_count = repo_count;
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


    fn move_by(&mut self, delta: isize) {
        let len = self.flat.len();
        if len == 0 {
            return;
        }
        self.selected = (self.selected as isize).saturating_add(delta).clamp(0, len as isize - 1) as usize;
        self.mark_selected_seen();
    }
}

/// Order a repo's worktrees so stacked branches follow their parent
/// (depth-first, keeping the configured sort among siblings), with the tree
/// prefix for each row. A child whose parent isn't visible stays top-level.
pub fn stack_order(wts: &[WorktreeView], stack: &std::collections::HashMap<String, String>) -> Vec<(usize, String)> {
    use std::collections::HashMap;
    let index: HashMap<&str, usize> = wts.iter().enumerate().map(|(i, v)| (v.worktree.id.as_str(), i)).collect();
    let parent_of = |i: usize| stack.get(&wts[i].worktree.id).and_then(|p| index.get(p.as_str()).copied()).filter(|&p| p != i);
    let mut children: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut roots = Vec::new();
    for i in 0..wts.len() {
        match parent_of(i) {
            Some(p) => children.entry(p).or_default().push(i),
            None => roots.push(i),
        }
    }
    let mut out = Vec::with_capacity(wts.len());
    let mut seen = vec![false; wts.len()];
    fn walk(
        i: usize,
        lead: &str,
        branch: &str,
        children: &HashMap<usize, Vec<usize>>,
        seen: &mut [bool],
        out: &mut Vec<(usize, String)>,
    ) {
        if std::mem::replace(&mut seen[i], true) {
            return;
        }
        out.push((i, format!("{lead}{branch}")));
        let kids = children.get(&i).map(Vec::as_slice).unwrap_or(&[]);
        let next_lead = if branch.is_empty() {
            String::new()
        } else if branch.starts_with('├') {
            format!("{lead}│  ")
        } else {
            format!("{lead}   ")
        };
        for (k, &c) in kids.iter().enumerate() {
            let b = if k + 1 == kids.len() { "└─ " } else { "├─ " };
            walk(c, &next_lead, b, children, seen, out);
        }
    }
    for r in roots {
        walk(r, "", "", &children, &mut seen, &mut out);
    }
    // Anything left (only possible with a cycle) is shown flat.
    out.extend(seen.iter().enumerate().filter(|(_, s)| !**s).map(|(i, _)| (i, String::new())));
    out
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

/// Backend-independent mouse events (mapped from crossterm in main.rs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mouse {
    /// Wheel: +1 down, -1 up.
    Scroll(i8),
    /// Left button down at a terminal cell; `double` for a quick second
    /// click on the same row.
    Click { x: u16, y: u16, double: bool },
    /// Pointer moved (no button held): drives the hover highlight.
    Move { x: u16, y: u16 },
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
    CtrlS,
}
