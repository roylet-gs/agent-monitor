//! Dashboard state and key handling. Pure: no I/O, so it is driven directly
//! in tests. Data arrives as a `Snapshot` from the collector (data.rs).

use am_core::model::{Repository, ReviewRequest, Subagent, WorktreeGroup, WorktreeView};
use am_core::settings::{SortKey, Settings};
use am_core::status::Status;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Health {
    pub github_failing: bool,
    pub linear_failing: bool,
    /// GitHub is failing because it rate-limited us (not because it's down).
    pub github_rate_limited: bool,
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
    /// Open PRs waiting on your review (`reviewRequests*` settings), newest
    /// first; `None` while off or before the first fetch/cache read.
    pub reviews: Option<Vec<ReviewRequest>>,
    /// Why the last review search failed (rate limit, gh logged out…);
    /// shown on the review page instead of "Loading…" or beside a cached list.
    pub review_error: Option<am_core::model::ReviewFetchError>,
    /// Autocomplete for review-group authors: who can author PRs in the
    /// tracked repos (looked up on GitHub) plus the review list's authors.
    pub review_people: Vec<am_core::model::ReviewPerson>,
    /// Open PRs requesting your or your teams' review on GitHub, ignoring the
    /// page's filters (only fetched while `reviewBadge` shows `all`).
    pub review_total: Option<u64>,
    /// Which PRs match each review group's GitHub search terms.
    pub review_query_hits: am_core::review_groups::QueryHits,
    /// Every tracked worktree's path, also those the hide* filters leave out
    /// (matching a cmux workspace to its most specific worktree).
    pub worktree_paths: Vec<String>,
    /// Worktrees the dashboard's filters leave out (the `─Worktrees` border's
    /// `N hidden`).
    pub hidden_worktrees: usize,
}

/// One number in the dashboard's review badge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BadgePart {
    /// A named group (`Platform 4`).
    Group(String, usize),
    /// The unnamed default group (`26 to review`).
    Rest(usize),
    /// Every open PR asking you or your teams on GitHub (`60 on GitHub`);
    /// `None` until a search has counted it.
    Total(Option<u64>),
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
    /// Fetch review requests now (the review page opened).
    RefreshReviews,
    /// `r` on the review page: clear the review caches and fetch again.
    ReloadReviews,
    /// Look up who can author PRs (review-group autocomplete).
    LoadReviewPeople,
    /// `ctrl-v`: the review page in a new terminal window (`am-tui --reviews`).
    OpenReviewWindow,
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
    /// Run the review script of a "pull + run script" job now.
    RunReviewScript { job: u64, repo_id: String, worktree_path: String },
    /// "Pull + run script" on a repo without a review script, after `y`:
    /// make it from the template, open it in the editor, and start the pull
    /// (the script runs once it's saved).
    AddReviewScriptAndPull(u64, Box<Repository>, Box<am_core::worktrees::CreateRequest>),
    /// `d` on an "Other sessions" row: stop tracking it.
    RemoveStandalone(String),
    AddRepo(String),
    /// Settings → Repositories… `s` / `r`: open the repo's startup or review script.
    EditScript(String, am_core::worktrees::ScriptKind),
    /// A repository was removed in Settings: refresh the dashboard.
    ReposChanged,
    InstallHooks,
    /// Settings → Run setup again: open the wizard (main.rs reads the repos).
    RunSetup,
    /// Settings → Reset settings, confirmed: reset settings.json, then run setup.
    ResetSettings,
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
    /// `v`: the review-requests page, in place of the worktree list (`V`:
    /// beside it).
    pub reviews: Option<crate::reviews_ui::ReviewPage>,
    /// `am-tui --reviews` (the window `ctrl-v` opens, or `am reviews`): only
    /// the review page, whatever `appMode` says. See [`App::reviews_only`].
    pub reviews_window: bool,
    /// How often each dashboard command ran since the last usage report
    /// (`Command::id` → count); main.rs drains it into `am.features`.
    pub command_counts: std::collections::BTreeMap<&'static str, u64>,
    /// The review page was opened by a mode change and wants a fetch (main
    /// takes it and asks the collector).
    pub review_fetch_due: bool,
    /// The review groups being edited (Review filters → Groups…), which the
    /// page previews in place of the saved ones.
    pub group_preview: Option<(Vec<am_core::review_groups::ReviewGroup>, am_core::review_groups::ReviewGroup)>,
    /// Select this worktree as soon as it shows up (after create).
    pub pending_select: Option<String>,
    /// Worktrees whose status changed since the user last had them selected.
    pub unseen: std::collections::HashSet<String>,
    seen_status: std::collections::HashMap<String, Option<Status>>,
    /// System sounds to play for this update (audio notifications).
    pub sounds: Vec<String>,
    /// Review requests already seen (by PR URL); `None` until the first list,
    /// which only fills it, so starting am doesn't announce everything.
    seen_reviews: Option<std::collections::HashSet<String>>,
    /// New review requests to post as cmux notifications (`cmuxSync`):
    /// (title, body).
    pub review_alerts: Vec<(String, String)>,
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
    /// Modifier keys held down (only terminals with the kitty keyboard
    /// protocol report them); the key bar names what `v` does with each.
    pub held: Held,
    /// The terminal reports modifiers on their own (`held` works), so the
    /// key bar can keep Shift/Ctrl variants out of sight until held.
    pub modifiers_reported: bool,
    /// What a click does where, recorded by the last `ui::draw` for the
    /// layer on top (so clicks always match what's on screen).
    pub hits: std::cell::RefCell<Vec<crate::ui::Hit>>,
    /// First visible row of the worktree list, kept between draws so the
    /// list only scrolls when the selection leaves the view.
    pub list_scroll: std::cell::Cell<usize>,
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
            reviews: None,
            reviews_window: false,
            command_counts: Default::default(),
            review_fetch_due: false,
            group_preview: None,
            pending_select: None,
            unseen: Default::default(),
            seen_status: Default::default(),
            sounds: Vec::new(),
            seen_reviews: None,
            review_alerts: Vec::new(),
            deleting: Default::default(),
            jobs: Default::default(),
            hover: None,
            held: Held::default(),
            modifiers_reported: false,
            hits: Default::default(),
            list_scroll: Default::default(),
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
        self.track_new_reviews();
        self.sync_editor();
        let n = self.review_list().len();
        if let Some(p) = &mut self.reviews {
            p.clamp(n);
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
        self.settings_editor = Some(crate::settings_ui::SettingsEditor::new(&self.settings));
        self.sync_editor();
    }

    /// Keep the settings editor's view of the app current: repo count,
    /// review-group autocomplete and counts.
    fn sync_editor(&mut self) {
        let candidates = self.group_candidates();
        let (per_group, hidden) = match &self.snapshot.reviews {
            Some(list) => crate::reviews_ui::counts(list, &self.snapshot.repos, self.grouping()),
            None => (Vec::new(), 0),
        };
        if let Some(e) = &mut self.settings_editor {
            e.repo_count = self.snapshot.repos.len();
            e.candidates = candidates;
            e.group_counts = crate::review_groups_ui::GroupCounts { per_group, hidden };
        }
    }

    /// What the review-group lists complete from: people, the teams asked in
    /// the list, and the tracked repos.
    fn group_candidates(&self) -> crate::review_groups_ui::Candidates {
        use am_core::model::ReviewPerson;
        let mut teams: Vec<ReviewPerson> = Vec::new();
        for r in self.snapshot.reviews.iter().flatten() {
            for t in r.requested.iter().filter(|w| w.contains('/')) {
                if !teams.iter().any(|p| p.login.eq_ignore_ascii_case(t)) {
                    teams.push(ReviewPerson { login: t.clone(), name: String::new() });
                }
            }
        }
        teams.sort_by_key(|p| p.login.to_lowercase());
        let repos = self
            .snapshot
            .repos
            .iter()
            .map(|r| {
                let slug = self.snapshot.reviews.iter().flatten().find(|x| x.repo_id == r.id).map(|x| x.repo.clone());
                ReviewPerson { login: r.name.clone(), name: slug.unwrap_or_default() }
            })
            .collect();
        crate::review_groups_ui::Candidates { people: self.snapshot.review_people.clone(), teams, repos }
    }

    /// The saved review groups, measured from now.
    pub fn grouping(&self) -> crate::reviews_ui::Grouping<'_> {
        let now = am_core::timefmt::now_unix();
        let hits = &self.snapshot.review_query_hits;
        match &self.group_preview {
            Some((groups, default)) => crate::reviews_ui::Grouping { groups, default, now, hits },
            None => crate::reviews_ui::Grouping::new(&self.settings, now, hits),
        }
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

    /// Queue a notification for each review request that wasn't listed
    /// before (one the review groups show, not one you already approved).
    /// Every listed PR counts as seen, shown or not, so one that a filter or
    /// group change brings into view isn't announced as new.
    fn track_new_reviews(&mut self) {
        let Some(all) = &self.snapshot.reviews else { return };
        let first = self.seen_reviews.is_none();
        let seen = self.seen_reviews.get_or_insert_with(Default::default);
        let new: std::collections::HashSet<String> = all.iter().filter(|r| seen.insert(r.pr.url.clone())).map(|r| r.pr.url.clone()).collect();
        if first || new.is_empty() || !self.settings.cmux_sync || self.reviews_window {
            return;
        }
        let alerts = self
            .review_list()
            .into_iter()
            .filter(|r| !r.approved_by_you && new.contains(&r.pr.url))
            .map(|r| (format!("Review requested: {} #{}", r.repo, r.pr.number), format!("{} — @{}", r.pr.title, r.author)));
        self.review_alerts.extend(alerts.collect::<Vec<_>>());
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
                    true => (am_core::model::TMP_SPACE_GROUP_ID.to_string(), am_core::model::TMP_SPACE_GROUP_NAME.to_string()),
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
        // Compact: no blank lines, and no ticket headers (the ticket column
        // names it). Neither takes a selection index.
        if self.settings.compact_view {
            self.rows.retain(|r| !matches!(r, Row::Spacer | Row::Ticket(_)));
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
        // The pane went away (its question was answered elsewhere, e.g. the
        // review script was saved): the keys go back to the page.
        if self.jobs.focused && !self.jobs_pane_shown() {
            self.jobs.focused = false;
        }
        // Ctrl+⏎ means "pull + run script" on the review page only; anywhere
        // else it's a plain ⏎.
        let page_has_keys = self.review_focused()
            && self.popup.is_none()
            && self.setup.is_none()
            && self.modal.is_none()
            && self.settings_editor.is_none()
            && !self.show_help;
        let key = if key == Key::CtrlEnter && !page_has_keys { Key::Enter } else { key };
        let action = self.handle_key(key);
        self.refresh_group_preview();
        action
    }

    /// While the review groups page is open, the page beneath shows its
    /// groups as edited so far (from the cached list, no fetch), counts too.
    fn refresh_group_preview(&mut self) {
        let preview = self.settings_editor.as_ref().and_then(|e| e.groups.as_ref()).map(|g| g.preview());
        if preview != self.group_preview {
            self.group_preview = preview;
            self.sync_editor();
        }
    }

    fn handle_key(&mut self, key: Key) -> Action {
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
                            let help = self.keymap.key(crate::keymap::Command::Help).map_or("?".into(), crate::keymap::display);
                            self.notice = Some(format!("Setup complete — press {help} for keys"));
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
                    // Its pull (and review script output) no longer matters.
                    self.jobs.forget_worktree(&id);
                    let job = self.jobs.start_delete(id, view.worktree.display_name());
                    Action::Delete(job, view, delete_branch)
                }
                // The browser stays open (showing "Adding…") until the result comes back.
                Effect::AddRepo(path) => Action::AddRepo(path),
                Effect::ResetSettings => {
                    self.modal = None;
                    Action::ResetSettings
                }
                Effect::SetTelemetry(on) => {
                    self.modal = None;
                    let choice = if on { am_core::settings::TelemetryChoice::On } else { am_core::settings::TelemetryChoice::Off };
                    match crate::settings_ui::persist("telemetry", serde_json::Value::from(choice.id())) {
                        Ok(s) => {
                            self.set_settings(s);
                            self.notice = Some(if on {
                                "Thanks! Usage reports on (Settings → Usage reports)".into()
                            } else {
                                "No usage reports (Settings → Usage reports)".into()
                            });
                            Action::SettingsChanged
                        }
                        Err(e) => {
                            self.notice = Some(e);
                            Action::None
                        }
                    }
                }
                Effect::AddReviewScript(r) => {
                    self.modal = None;
                    match self.start_review_pull(&r) {
                        Some((job, repo, req)) => Action::AddReviewScriptAndPull(job, Box::new(repo), Box::new(req)),
                        None => Action::None,
                    }
                }
            };
        }
        if let Some(editor) = &mut self.settings_editor {
            let outcome = editor.on_key(key);
            return self.editor_outcome(outcome);
        }
        if self.show_help {
            if matches!(key, Key::Esc | Key::Char('q')) || self.keymap.command(key) == Some(crate::keymap::Command::Help) {
                self.show_help = false;
            }
            return Action::None;
        }
        if self.reviews.as_ref().is_some_and(|p| !p.split) && !self.jobs.focused {
            return self.review_key(key);
        }
        if self.jobs.focused {
            use crate::jobs::JobEffect;
            let effect = self.jobs.on_key(key);
            self.rebuild_rows();
            return match effect {
                JobEffect::None => Action::None,
                JobEffect::Create(job, repo, req) => Action::Create(job, repo, req),
                JobEffect::RunScript { job, repo_id, worktree_path } => Action::RunScript { job, repo_id, worktree_path },
                JobEffect::RunReviewScript { job, repo_id, worktree_path } => Action::RunReviewScript { job, repo_id, worktree_path },
            };
        }
        if self.review_focused() {
            return self.review_key(key);
        }
        self.dashboard_key(key)
    }

    /// Whether the jobs pane is on screen. The full review page keeps it out
    /// of the way (pulls and removals show on their rows) unless a job waits
    /// for an answer, which can only be given there.
    pub fn jobs_pane_shown(&self) -> bool {
        match &self.reviews {
            Some(p) if !p.split => self.jobs.waiting(),
            _ => self.jobs.visible(),
        }
    }

    /// Whether the review page takes the keys: a full page, or the review
    /// side of a split while it has focus, and the jobs pane doesn't.
    pub fn review_focused(&self) -> bool {
        self.reviews.as_ref().is_some_and(|p| !p.split || p.focused) && !self.jobs.focused
    }

    /// Give the keys to the jobs pane (Tab, a click on it).
    fn focus_jobs(&mut self) {
        if let Some(p) = &mut self.reviews {
            p.focused = false;
        }
        self.jobs.focus();
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
            Outcome::EditScript(id, kind) => Action::EditScript(id, kind),
            Outcome::ReposChanged => Action::ReposChanged,
            Outcome::RunSetup => Action::RunSetup,
            Outcome::ViewLogs => Action::ViewLogs,
            Outcome::LoadPeople => Action::LoadReviewPeople,
            Outcome::ConfirmReset => {
                self.modal = Some(crate::modals::Modal::ConfirmReset);
                Action::None
            }
        }
    }

    fn dashboard_key(&mut self, key: Key) -> Action {
        // Tab: list → review pane (split) → jobs pane → list.
        if key == Key::Tab {
            if let Some(p) = &mut self.reviews {
                p.focused = true;
            } else if self.jobs.visible() {
                self.focus_jobs();
            }
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
        // For the `am.features` usage report (main.rs drains it).
        *self.command_counts.entry(cmd.id()).or_default() += 1;
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
            // Only the review page pulls (its selected PR).
            C::PullToTmpSpace | C::PullAndRunScript => {}
            C::Filter => {
                self.settings_editor =
                    Some(crate::settings_ui::SettingsEditor::only(&self.settings, crate::settings_ui::WORKTREE_FILTERS));
                self.sync_editor();
            }
            C::ToggleFilters => return self.toggle_filters(),
            C::AddRepo => self.modal = Some(self.add_repo_modal()),
            C::ReviewRequests => match &mut self.reviews {
                // From the split: the page takes the whole body.
                Some(p) => {
                    p.split = false;
                    p.focused = true;
                }
                None => return self.open_reviews(false),
            },
            C::ReviewSplit => match self.reviews.as_ref().map(|p| p.split) {
                Some(true) => self.reviews = None,
                Some(false) => {}
                None => return self.open_reviews(true),
            },
            C::ReviewWindow => {
                if self.reviews_refused() {
                    return Action::None;
                }
                self.notice = Some("Opening review requests in a new window…".into());
                return Action::OpenReviewWindow;
            }
            C::Settings => self.open_settings(),
            C::MoveDown => self.move_by(1),
            C::MoveUp => self.move_by(-1),
            C::PageDown => self.move_by(crate::nav::PAGE),
            C::PageUp => self.move_by(-crate::nav::PAGE),
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
        let action = self.handle_mouse(m);
        self.refresh_group_preview();
        action
    }

    fn handle_mouse(&mut self, m: Mouse) -> Action {
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
                self.focus_jobs();
                Action::None
            }
            Target::JobKey(job, k) => {
                self.focus_jobs();
                self.jobs.cursor = job;
                self.on_key(k)
            }
            Target::Worktree(i) => {
                self.jobs.focused = false;
                if let Some(p) = &mut self.reviews {
                    p.focused = false;
                }
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
            L::Authors => {
                if let Some(e) = &mut self.settings_editor
                    && let Some(form) = e.groups.as_mut().and_then(|g| g.form.as_mut())
                {
                    form.accept(i, &e.candidates);
                }
                return Action::None;
            }
            L::Groups => {
                let Some(e) = &mut self.settings_editor else { return Action::None };
                let Some(g) = &mut e.groups else { return Action::None };
                let key = match &mut g.form {
                    Some(form) => (std::mem::replace(&mut form.cursor, i) == i && form.editing.is_none()).then_some(Key::Enter),
                    None => (std::mem::replace(&mut g.cursor, i) == i).then_some(Key::Enter),
                };
                g.confirm_remove = None;
                let Some(key) = key else { return Action::None };
                let outcome = e.on_key(key);
                return self.editor_outcome(outcome);
            }
            L::Sort | L::Multi | L::Plugins | L::Repos | L::RepoScripts | L::Keys | L::ThemeTokens | L::Palette => {
                let Some(e) = &mut self.settings_editor else { return Action::None };
                let at = |cursor: &mut usize| std::mem::replace(cursor, i) == i;
                match list {
                    L::Sort => e.sort.as_mut().and_then(|s| at(&mut s.cursor).then_some(Key::Char(' '))),
                    L::Multi => e.multi.as_mut().and_then(|m| at(&mut m.cursor).then_some(Key::Char(' '))),
                    L::Plugins => e.plugins.as_mut().and_then(|p| at(&mut p.cursor).then_some(Key::Char(' '))),
                    L::RepoScripts => e.repos.as_mut().and_then(|r| {
                        r.message = None;
                        let mut cur = r.scripts?;
                        let again = at(&mut cur);
                        r.scripts = Some(cur);
                        again.then_some(Key::Enter)
                    }),
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
                    let which = [FormField::Repo, FormField::Branch, FormField::Name, FormField::Base][i.min(3)];
                    // The repo row cycles on a second click.
                    let again = std::mem::replace(&mut f.field, which) == which;
                    (again && which == FormField::Repo).then_some(Key::Right)
                }
                _ => None,
            },
            L::SetupMode => {
                let Some(s) = &mut self.setup else { return Action::None };
                // A click picks; a second one goes on.
                (std::mem::replace(&mut s.reviews_mode, i == 1) == (i == 1)).then_some(Key::Enter)
            }
            L::SetupPrivacy => {
                let Some(s) = &mut self.setup else { return Action::None };
                // A click picks; a second one finishes.
                (std::mem::replace(&mut s.telemetry, i == 0) == (i == 0)).then_some(Key::Enter)
            }
            L::SetupReviews => {
                let Some(s) = &mut self.setup else { return Action::None };
                (std::mem::replace(&mut s.review.field, i) == i).then_some(Key::Char(' '))
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
            L::Reviews => {
                let Some(p) = &mut self.reviews else { return Action::None };
                // A click on the unfocused split side only focuses it.
                let was_focused = (!p.split || p.focused) && !self.jobs.focused;
                p.focused = true;
                self.jobs.focused = false;
                (std::mem::replace(&mut p.cursor, i) == i && was_focused).then_some(Key::Enter)
            }
            L::Jobs => {
                self.focus_jobs();
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
    /// `F`: the `f` filters off, or back on (`worktreeFiltersEnabled`); the
    /// base `hide*` filters always apply.
    fn toggle_filters(&mut self) -> Action {
        let on = !self.settings.worktree_filters_enabled;
        match crate::settings_ui::persist("worktreeFiltersEnabled", serde_json::Value::Bool(on)) {
            Ok(s) => {
                am_core::info!("tui", "Worktree filters turned {}", if on { "on" } else { "off" });
                let key = self.keymap.key(crate::keymap::Command::ToggleFilters).map_or("F".into(), crate::keymap::display);
                self.notice = Some(if on {
                    "Filters on".into()
                } else {
                    format!("Filters off — base filters only ({key} turns them back on)")
                });
                self.set_settings(s);
                Action::SettingsChanged
            }
            Err(e) => {
                self.notice = Some(e);
                Action::None
            }
        }
    }

    pub fn set_settings(&mut self, settings: Settings) {
        let to_reviews_mode = settings.reviews_mode() && !self.settings.reviews_mode();
        self.keymap = crate::keymap::Keymap::from_settings(&settings);
        self.settings = settings;
        if to_reviews_mode && self.setup.is_none() {
            self.enter_reviews_mode();
        }
        if let Some(e) = &mut self.settings_editor {
            let cursor = e.cursor;
            // The agent-plugins, key-bindings and review-groups pages report
            // changes while they stay open.
            let plugins = e.plugins.take();
            let groups = e.groups.take();
            let keys = e.keys.take();
            let repos = e.repos.take();
            let multi = e.multi.take();
            let repo_count = e.repo_count;
            let filter_box = e.is_filter_box().then_some(e.only).flatten();
            *e = crate::settings_ui::SettingsEditor::new(&self.settings);
            if filter_box.is_some() {
                e.only = filter_box;
            }
            // A mode change can hide the row the cursor was on.
            e.cursor = if e.fields().contains(&cursor) { cursor } else { e.fields().first().copied().unwrap_or(0) };
            e.plugins = plugins;
            e.keys = keys;
            e.repos = repos;
            e.multi = multi;
            e.groups = groups;
            e.repo_count = repo_count;
        }
        let snap = self.snapshot.clone();
        self.apply(snap);
    }

    /// The review requests in page order (empty while off or not loaded).
    pub fn review_list(&self) -> Vec<&ReviewRequest> {
        let Some(list) = &self.snapshot.reviews else { return Vec::new() };
        crate::reviews_ui::order(list, &self.snapshot.repos, self.grouping())
            .into_iter()
            .map(|i| &list[i])
            .collect()
    }

    pub fn selected_review(&self) -> Option<&ReviewRequest> {
        let cursor = self.reviews.as_ref()?.cursor;
        self.review_list().get(cursor).copied()
    }

    /// The dashboard badge's parts: each group shown on the dashboard with
    /// its count, then the GitHub total when on. `None` while off or before
    /// the list loads.
    pub fn review_badge_parts(&self) -> Option<Vec<BadgePart>> {
        let list = self.snapshot.reviews.as_ref().filter(|_| self.settings.review_requests_on())?;
        let g = self.grouping();
        let (per, _) = crate::reviews_ui::counts(list, &self.snapshot.repos, g);
        let mut parts = Vec::new();
        for (i, group) in g.groups.iter().enumerate().filter(|(_, x)| x.badge && x.enabled) {
            let name = if group.name.trim().is_empty() { format!("Group {}", i + 1) } else { group.name.trim().to_string() };
            parts.push(BadgePart::Group(name, per[i]));
        }
        if g.default.badge && g.default.enabled {
            let n = per[g.groups.len()];
            parts.push(match g.default.name.trim() {
                "" => BadgePart::Rest(n),
                name => BadgePart::Group(name.to_string(), n),
            });
        }
        if self.settings.review_badge_total {
            parts.push(BadgePart::Total(self.snapshot.review_total));
        }
        Some(parts)
    }

    /// Review requests are off: say so (and how to turn them on).
    fn reviews_refused(&mut self) -> bool {
        if !self.settings.gh_pr_status {
            self.notice = Some("Review requests need GitHub PR status (Settings → Fetch PRs)".into());
            return true;
        }
        if !self.settings.review_requests_enabled {
            self.notice = Some("Review requests are off (Settings → Show review requests)".into());
            return true;
        }
        false
    }

    /// Only the review page: a `--reviews` window, or review-only mode
    /// (`appMode: "reviews"`). Leaving the page quits.
    pub fn reviews_only(&self) -> bool {
        self.reviews_window || self.settings.reviews_mode()
    }

    /// Review-only mode (at start-up, after setup, or switched on in
    /// Settings): the full review page, even when review requests are off
    /// (the page then says how to turn them on), and a fetch.
    pub fn enter_reviews_mode(&mut self) {
        self.jobs.focused = false;
        self.reviews = Some(crate::reviews_ui::ReviewPage { focused: true, ..Default::default() });
        self.review_fetch_due = self.settings.review_requests_on();
    }

    /// The review page, in place of the list or (`split`) beside it, with
    /// the keys.
    pub fn open_reviews(&mut self, split: bool) -> Action {
        if self.reviews_refused() {
            return Action::None;
        }
        self.jobs.focused = false;
        self.reviews = Some(crate::reviews_ui::ReviewPage { split, focused: true, ..Default::default() });
        Action::RefreshReviews
    }

    /// Leave the review page: closes it, or quits a review-only window.
    fn close_reviews(&mut self) -> Action {
        if self.reviews_only() {
            return Action::Quit;
        }
        self.reviews = None;
        Action::None
    }

    /// Whether a review-page key applies to the selected PR (the key bar
    /// dims the ones that don't).
    pub fn review_key_available(&self, key: Key) -> bool {
        use crate::keymap::Command as C;
        let r = self.selected_review();
        match self.keymap.command(key) {
            _ if key == Key::Enter => r.is_some(),
            Some(C::OpenLinear) => r.is_some_and(|r| r.linear.is_some()),
            Some(C::Delete) => r.and_then(|r| r.worktree_id.as_deref()).is_some_and(|id| !self.is_deleting(id)),
            Some(C::PullToTmpSpace) => r.is_some_and(|r| {
                !r.is_cross_repository
                    && self.review_pull(r).is_none_or(|j| matches!(j.state, crate::jobs::JobState::Failed(_)))
            }),
            Some(C::PullAndRunScript) => r.is_some_and(|r| {
                !r.is_cross_repository
                    && r.worktree_id.is_none()
                    && self.review_pull(r).is_none_or(|j| matches!(j.state, crate::jobs::JobState::Failed(_)))
            }),
            _ => true,
        }
    }

    /// Keys on the review page. Rebound dashboard keys carry over (move,
    /// open PR/ticket, pull, refresh, help, settings); Esc, `q` and the
    /// page's own key go back.
    fn review_key(&mut self, key: Key) -> Action {
        use crate::keymap::Command as C;
        if key == Key::CtrlC {
            return Action::Quit;
        }
        if key == Key::Enter {
            return self.review_open();
        }
        if key == Key::CtrlEnter {
            return self.pull_review(true);
        }
        let len = self.review_list().len();
        let cmd = self.keymap.command(key);
        let reviews_only = self.reviews_only();
        let Some(page) = &mut self.reviews else { return Action::None };
        // Tab in a split: on to the jobs pane when it's open, else the list.
        // On the full page: to the jobs pane while it shows (a question
        // waits), and back with its own Tab.
        let pane = match page.split {
            true => self.jobs.visible(),
            false => self.jobs.waiting(),
        };
        if key == Key::Tab && (page.split || pane) {
            page.focused = false;
            if pane {
                self.jobs.focus();
            }
            return Action::None;
        }
        match cmd {
            Some(C::Quit) => return self.close_reviews(),
            // `v` in a split takes the whole body; on the full page it goes back.
            Some(C::ReviewRequests) if page.split => page.split = false,
            Some(C::ReviewRequests) => return self.close_reviews(),
            Some(C::ReviewSplit) if page.split => self.reviews = None,
            Some(C::ReviewSplit) if !reviews_only => page.split = true,
            Some(C::ReviewWindow) if !reviews_only => return self.run_command(C::ReviewWindow),
            Some(C::MoveDown) => page.move_by(1, len),
            Some(C::MoveUp) => page.move_by(-1, len),
            Some(C::PageDown) => page.move_by(crate::nav::PAGE, len),
            Some(C::PageUp) => page.move_by(-crate::nav::PAGE, len),
            Some(C::Top) => page.move_by(isize::MIN / 2, len),
            Some(C::Bottom) => page.move_by(isize::MAX / 2, len),
            Some(C::Open) => return self.review_open(),
            Some(C::OpenPr) => return self.review_open_pr(),
            Some(C::OpenLinear) => {
                let url = self.selected_review().and_then(|r| r.linear.as_ref()).map(|l| linear_url(&l.url, &self.settings));
                match url {
                    Some(url) => return Action::OpenUrl(url),
                    None => self.notice = Some("No Linear ticket for this PR".into()),
                }
            }
            Some(C::PullToTmpSpace) => return self.pull_review(false),
            Some(C::PullAndRunScript) => return self.pull_review(true),
            Some(C::Delete) => self.remove_review_worktree(),
            // `f` opens the review filters here.
            Some(C::Filter) => {
                self.settings_editor = Some(crate::settings_ui::SettingsEditor::only(&self.settings, crate::settings_ui::REVIEW_FILTERS));
                self.sync_editor();
            }
            Some(C::Refresh) => {
                self.notice = Some("Refreshing review requests (cache cleared)…".into());
                return Action::ReloadReviews;
            }
            Some(C::Help) => self.show_help = true,
            Some(C::Settings) => self.open_settings(),
            _ => {}
        }
        Action::None
    }

    /// The pulled worktree for the selected PR, unless it's being removed.
    pub fn review_worktree(&self) -> Option<&WorktreeView> {
        let id = self.selected_review()?.worktree_id.as_deref()?;
        if self.is_deleting(id) {
            return None;
        }
        self.snapshot.groups.iter().flat_map(|g| &g.worktrees).find(|v| v.worktree.id == id)
    }

    /// ⏎ (or a click on the selected row) on the review page: a pulled PR
    /// opens its worktree in the IDE, like ⏎ on the dashboard; one that isn't
    /// pulled is pulled into Reviewing like `t`, and a fork's opens the PR.
    fn review_open(&mut self) -> Action {
        if let Some(v) = self.review_worktree() {
            return Action::Open(Box::new(v.clone()));
        }
        let Some(r) = self.selected_review().cloned() else { return Action::None };
        if r.is_cross_repository {
            return self.review_open_pr();
        }
        if r.worktree_id.is_some() {
            self.notice = Some(format!("{} is being removed", r.head_ref_name));
            return Action::None;
        }
        if self.review_pull(&r).is_some_and(|j| !matches!(j.state, crate::jobs::JobState::Failed(_))) {
            self.notice = Some(format!("{} is already being pulled", r.head_ref_name));
            return Action::None;
        }
        self.pull_review(false)
    }

    fn review_open_pr(&mut self) -> Action {
        match self.selected_review() {
            Some(r) => Action::OpenUrl(r.pr.url.clone()),
            None => Action::None,
        }
    }

    /// Check the PR's branch out into Reviewing (the tmp space), or jump to
    /// the worktree that already has it. With `run_script` ("pull + run
    /// script") the repo's review script runs in the new worktree; that only
    /// happens on a fresh pull.
    fn pull_review(&mut self, run_script: bool) -> Action {
        let Some(r) = self.selected_review().cloned() else { return Action::None };
        if r.is_cross_repository {
            self.notice = Some(format!("#{} is from a fork — its branch isn't on origin", r.pr.number));
            return Action::None;
        }
        if run_script && r.worktree_id.is_some() {
            self.notice = Some(format!("{} is already pulled — pull + run script only runs on a fresh pull", r.head_ref_name));
            return Action::None;
        }
        if let Some(id) = r.worktree_id {
            // A review-only window has no list to show it in.
            if self.reviews_only() {
                self.notice = Some(format!("{} is already checked out", r.head_ref_name));
                return Action::None;
            }
            // A split stays open with the list taking the keys; the full
            // page makes way for the list.
            match &mut self.reviews {
                Some(p) if p.split => p.focused = false,
                _ => self.reviews = None,
            }
            match self.flat.iter().position(|v| v.worktree.id == id) {
                Some(i) => {
                    self.selected = i;
                    self.mark_selected_seen();
                }
                None => self.pending_select = Some(id),
            }
            self.notice = Some(format!("{} is already checked out", r.head_ref_name));
            return Action::None;
        }
        let Some(repo) = self.snapshot.repos.iter().find(|x| x.id == r.repo_id).cloned() else {
            self.notice = Some(format!("{} isn't tracked any more", r.repo));
            return Action::None;
        };
        // Stay on the page so several can be pulled in a row; the row shows
        // the job. A failed pull can be retried.
        match self.review_pull(&r).map(|j| (j.id, matches!(j.state, crate::jobs::JobState::Failed(_)))) {
            Some((id, true)) => self.jobs.dismiss(id),
            Some(_) => {
                self.notice = Some(format!("{} is already being pulled", r.head_ref_name));
                return Action::None;
            }
            None => {}
        }
        // No review script yet: offer to add one first.
        if run_script && !am_core::worktrees::ScriptKind::Review.exists(&repo.id) {
            self.modal = Some(crate::modals::Modal::AddReviewScript { review: Box::new(r), repo: repo.name });
            return Action::None;
        }
        if run_script {
            return match self.start_review_pull(&r) {
                Some((job, repo, req)) => Action::Create(job, Box::new(repo), Box::new(req)),
                None => Action::None,
            };
        }
        let req = am_core::worktrees::CreateRequest {
            branch: r.head_ref_name.clone(),
            mode: am_core::worktrees::CreateMode::TrackRemote,
            tmp_space: true,
            ..Default::default()
        };
        self.notice = Some(format!("Pulling {} into Reviewing…", r.head_ref_name));
        let job = self.jobs.start_create(repo.clone(), req.clone());
        self.rebuild_rows();
        Action::Create(job, Box::new(repo), Box::new(req))
    }

    /// Start a "pull + run script" job for `r` (checked by `pull_review`).
    fn start_review_pull(&mut self, r: &ReviewRequest) -> Option<(u64, Repository, am_core::worktrees::CreateRequest)> {
        let repo = self.snapshot.repos.iter().find(|x| x.id == r.repo_id).cloned()?;
        let req = am_core::worktrees::CreateRequest {
            branch: r.head_ref_name.clone(),
            mode: am_core::worktrees::CreateMode::TrackRemote,
            tmp_space: true,
            ..Default::default()
        };
        self.notice = Some(format!("Pulling {} into Reviewing, then running its review script…", r.head_ref_name));
        let job = self.jobs.start_review_pull(repo.clone(), req.clone(), review_script_env(r));
        self.rebuild_rows();
        Some((job, repo, req))
    }

    /// `d` on a pulled PR: the usual delete dialog for its worktree, over
    /// the page; the row reads "removing…" until it's gone.
    fn remove_review_worktree(&mut self) {
        let Some(r) = self.selected_review().cloned() else { return };
        let Some(id) = r.worktree_id else {
            self.notice = Some(format!("#{} isn't pulled", r.pr.number));
            return;
        };
        if self.is_deleting(&id) {
            self.notice = Some(format!("{} is already being removed", r.head_ref_name));
            return;
        }
        let view = self.snapshot.groups.iter().flat_map(|g| &g.worktrees).find(|v| v.worktree.id == id).cloned();
        match view {
            Some(v) => self.modal = Some(crate::modals::Modal::delete(v)),
            None => self.notice = Some(format!("{} isn't in the worktree list", r.head_ref_name)),
        }
    }

    /// The job pulling `r`'s branch into Reviewing (running, waiting, failed,
    /// or done but not in the snapshot yet). `None` once it's pulled.
    pub fn review_pull(&self, r: &ReviewRequest) -> Option<&crate::jobs::Job> {
        if r.worktree_id.is_some() {
            return None;
        }
        self.jobs.list.iter().rev().find(|j| {
            matches!(&j.kind, crate::jobs::JobKind::Create { repo, req, .. }
                if req.tmp_space && repo.id == r.repo_id && req.branch == r.head_ref_name)
        })
    }

    /// The job running the review script in `r`'s pulled worktree (or whose
    /// script failed). `review_pull` stops seeing the job once the worktree
    /// is listed; this picks it up from there.
    pub fn review_script_job(&self, r: &ReviewRequest) -> Option<&crate::jobs::Job> {
        let id = r.worktree_id.as_deref()?;
        self.jobs.list.iter().rev().find(|j| {
            matches!(&j.kind, crate::jobs::JobKind::Create { worktree_id: Some(w), review_script: Some(_), .. } if w == id)
                && matches!(j.state, crate::jobs::JobState::Running | crate::jobs::JobState::Waiting | crate::jobs::JobState::Failed(_))
        })
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
        self.selected = crate::nav::moved(self.selected, delta, len);
        self.mark_selected_seen();
    }
}

/// Order a repo's worktrees so stacked branches follow their parent
/// (depth-first, keeping the configured sort among siblings), with the tree
/// prefix for each row. A child whose parent isn't visible stays top-level.
pub fn stack_order(wts: &[WorktreeView], stack: &std::collections::HashMap<String, String>) -> Vec<(usize, String)> {
    use std::collections::HashMap;
    let index: HashMap<&str, usize> = wts.iter().enumerate().map(|(i, v)| (v.worktree.id.as_str(), i)).collect();
    tree_order(wts.len(), |i| stack.get(&wts[i].worktree.id).and_then(|p| index.get(p.as_str()).copied()))
}

/// Order `0..len` so each item follows its parent (depth-first, keeping the
/// given order among siblings), with its tree prefix ("", "├─ ", "│  └─ "…).
/// An item whose parent is itself or part of a cycle is shown flat.
pub fn tree_order(len: usize, parent_of: impl Fn(usize) -> Option<usize>) -> Vec<(usize, String)> {
    use std::collections::HashMap;
    let parent_of = |i: usize| parent_of(i).filter(|&p| p != i && p < len);
    let mut children: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut roots = Vec::new();
    for i in 0..len {
        match parent_of(i) {
            Some(p) => children.entry(p).or_default().push(i),
            None => roots.push(i),
        }
    }
    let mut out = Vec::with_capacity(len);
    let mut seen = vec![false; len];
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

/// What the review script is told about the PR (`AM_WORKTREE_PATH` is
/// added once the worktree exists).
fn review_script_env(r: &ReviewRequest) -> Vec<(String, String)> {
    [
        ("AM_PR_NUMBER", r.pr.number.to_string()),
        ("AM_PR_URL", r.pr.url.clone()),
        ("AM_PR_TITLE", r.pr.title.clone()),
        ("AM_PR_AUTHOR", r.author.clone()),
        ("AM_PR_BRANCH", r.head_ref_name.clone()),
        ("AM_PR_BASE", r.pr.base_ref_name.clone().unwrap_or_default()),
        ("AM_REPO", r.repo.clone()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
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

/// Modifier keys held on their own (mapped from crossterm in main.rs).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Held {
    pub shift: bool,
    pub ctrl: bool,
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
    /// Only told apart from Enter where the terminal speaks the kitty
    /// keyboard protocol; elsewhere it arrives as `Enter`.
    CtrlEnter,
    Esc,
    CtrlC,
    CtrlD,
    CtrlU,
    CtrlS,
    CtrlV,
}
