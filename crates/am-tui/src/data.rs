//! Background collector: builds `Snapshot`s off the UI thread.
//!
//! Change detection is `PRAGMA data_version` on a long-lived connection —
//! it bumps whenever any *other* connection (a hook process) commits, so the
//! dashboard reacts to agent events within ~200 ms without owning the pub/sub
//! socket (which the TS TUI may hold during the migration) and without
//! rescanning git. Git is refreshed for the worktrees whose agent just did
//! something, plus a full sweep every polling interval.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use am_core::integrations::reviews::ReviewRepo;
use am_core::integrations::{Integrations, RepoBranches};
use am_core::harness::HarnessKind;
use am_core::model::{Ide, LinearInfo, PrInfo, ReviewFetchError, ReviewPerson, ReviewRequest, RunningProcess, SessionView, Subagent, Worktree, WorktreeView};
use am_core::status::Status;
use am_core::settings::Settings;
use am_core::context::ContextUsage;
use am_core::perf::Perf;
use am_core::usage::DailyUsage;
use am_core::{db, debug, gitinfo, grouping, info, procscan, stacks, warn, worktrees};
use rusqlite::Connection;

use crate::app::{Health, Snapshot};
use crate::gitwork::{self, Tracker};

const TICK: Duration = Duration::from_millis(200);
/// A native process snapshot costs ~2 ms, so terminals/editors/dev servers
/// are re-checked often, independent of the (slower) git sweep.
const PROCESS_INTERVAL: Duration = Duration::from_secs(2);
/// Pick up worktrees created/removed outside am (`git worktree add`, other
/// tools): one `git worktree list` per repo.
const SYNC_INTERVAL: Duration = Duration::from_secs(10);
/// Re-read the transcripts' appended bytes for today's token total.
const USAGE_INTERVAL: Duration = Duration::from_secs(10);
/// Recently finished subagents stay visible this long.
const RECENT_SUBAGENT_MINUTES: u32 = 30;
/// How far back a stopped subagent can still be "parked" (waiting on a
/// background command); matches the stale cut-off for running ones.
const PARKED_SUBAGENT_MINUTES: u32 = 120;
/// The tool a background subagent calls to hand its final report back.
const HANDBACK_TOOL: &str = "SubagentHandback";

enum Cmd {
    Refresh { force: bool },
    /// Fetch review requests now (the review page opened).
    RefreshReviews,
    /// Look up who can author PRs (autocomplete for review groups).
    RefreshPeople,
    /// Clear the review caches, then fetch again (`r` on the review page).
    ReloadReviews,
    Settings(Box<Settings>),
    Quit,
}

type Fields = HashMap<(String, String), (Option<PrInfo>, Option<LinearInfo>)>;

/// Sent by the integrations thread after hydrating and after every refresh.
struct IntegrationUpdate {
    fields: Fields,
    health: Health,
    reviews: Option<Vec<ReviewRequest>>,
    review_error: Option<ReviewFetchError>,
    people: Vec<ReviewPerson>,
    review_total: Option<u64>,
    review_hits: am_core::review_groups::QueryHits,
}

pub struct Collector {
    rx: Receiver<Snapshot>,
    cmd: Sender<Cmd>,
    int_cmd: Sender<Cmd>,
    handle: Option<JoinHandle<()>>,
}

impl Collector {
    pub fn spawn(settings: Settings) -> Collector {
        let (tx, rx) = mpsc::channel();
        let (cmd, cmd_rx) = mpsc::channel();
        let (int_tx, int_rx) = mpsc::channel();
        let (int_cmd, int_cmd_rx) = mpsc::channel();
        let int_settings = settings.clone();
        std::thread::Builder::new()
            .name("integrations".into())
            .spawn(move || run_integrations(int_settings, int_cmd_rx, int_tx))
            .expect("spawn integrations");
        let handle = std::thread::Builder::new()
            .name("collector".into())
            .spawn(move || {
                let mut settings = settings;
                let Some(conn) = open_db("tui", &cmd_rx, &mut settings, db::open) else { return };
                Worker::new(settings, conn, tx, int_rx).run(cmd_rx);
            })
            .expect("spawn collector");
        Collector { rx, cmd, int_cmd, handle: Some(handle) }
    }

    pub fn try_recv(&self) -> Option<Snapshot> {
        // Coalesce: only the newest snapshot matters.
        self.rx.try_iter().last()
    }

    pub fn update_settings(&self, settings: Settings) {
        let _ = self.cmd.send(Cmd::Settings(Box::new(settings.clone())));
        let _ = self.int_cmd.send(Cmd::Settings(Box::new(settings)));
    }

    pub fn refresh(&self, force: bool) {
        let _ = self.cmd.send(Cmd::Refresh { force });
        let _ = self.int_cmd.send(Cmd::Refresh { force });
    }

    /// Fetch review requests now (the review page opened).
    pub fn refresh_reviews(&self) {
        let _ = self.int_cmd.send(Cmd::RefreshReviews);
    }

    /// Look up who can author PRs in the tracked repos (at most hourly).
    pub fn refresh_people(&self) {
        let _ = self.int_cmd.send(Cmd::RefreshPeople);
    }

    /// Clear the review caches and fetch the list again.
    pub fn reload_reviews(&self) {
        let _ = self.int_cmd.send(Cmd::ReloadReviews);
    }

    pub fn shutdown(mut self) {
        // The integrations thread may be mid-`gh` call; it exits on its next
        // loop and holds nothing that needs flushing, so it is not joined.
        let _ = self.int_cmd.send(Cmd::Quit);
        let _ = self.cmd.send(Cmd::Quit);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

#[derive(Default, Clone, PartialEq)]
struct ProcData {
    has_terminal: bool,
    open_ide: Option<Ide>,
    running: Vec<RunningProcess>,
    /// Claude Code shell commands still running here (see `parked_subagents`).
    claude_shells: usize,
    /// Those of `claude_shells` matching `runningProcessFilter`: the row's
    /// `$ N commands` highlight (none without a filter).
    matching_shells: usize,
    /// Agent CLI processes here by harness id (see `live_sessions`).
    agents: HashMap<&'static str, usize>,
}

struct Worker {
    settings: Settings,
    conn: Connection,
    tx: Sender<Snapshot>,
    integrations: Receiver<IntegrationUpdate>,
    fields: Fields,
    health: Health,
    reviews: Option<Vec<ReviewRequest>>,
    review_error: Option<ReviewFetchError>,
    people: Vec<ReviewPerson>,
    review_total: Option<u64>,
    review_hits: am_core::review_groups::QueryHits,
    /// Git status per worktree; the git itself runs on the `git` thread.
    git: Tracker,
    git_jobs: Sender<Vec<gitwork::Job>>,
    git_done: Receiver<gitwork::Batch>,
    procs: HashMap<String, ProcData>,
    /// Context usage per worktree id, re-read with the git data.
    ctx: HashMap<String, Option<ContextUsage>>,
    /// Today's token usage, read incrementally (only while `showDailyTokens`).
    usage: DailyUsage,
    last_usage: Option<Instant>,
    last_procs: Option<Instant>,
    last_sync: Option<Instant>,
    /// Per repo: (branch-tips signature, trunk ref, git view) — rev-lists only
    /// rerun when some branch tip moved.
    stack_git: HashMap<String, (String, stacks::GitView)>,
    agent_seen: HashMap<String, String>,
    last_full: Option<Instant>,
    data_version: i64,
    last_sent: Option<Snapshot>,
    /// Parked subagent ids last cycle, to log only when the set changes.
    parked: HashSet<String>,
    /// Newest change in the plugin dir; a new value reloads the registry (a
    /// manifest was added, edited or removed while the TUI runs).
    plugins_stamp: Option<std::time::SystemTime>,
    /// Stage timings (`am perf`).
    perf: Perf,
}

impl Worker {
    fn new(settings: Settings, conn: Connection, tx: Sender<Snapshot>, integrations: Receiver<IntegrationUpdate>) -> Worker {
        let (git_jobs, git_done) = gitwork::spawn();
        Worker {
            settings,
            conn,
            tx,
            integrations,
            fields: HashMap::new(),
            health: Health::default(),
            reviews: None,
            review_error: None,
            people: Vec::new(),
            review_total: None,
            review_hits: Default::default(),
            git: Tracker::default(),
            git_jobs,
            git_done,
            procs: HashMap::new(),
            ctx: HashMap::new(),
            usage: DailyUsage::new(),
            last_usage: None,
            last_procs: None,
            parked: HashSet::new(),
            plugins_stamp: am_core::harness::plugins_stamp(),
            last_sync: None,
            stack_git: HashMap::new(),
            agent_seen: HashMap::new(),
            last_full: None,
            data_version: -1,
            last_sent: None,
            perf: Perf::new(),
        }
    }

    fn poll_interval(&self) -> Duration {
        Duration::from_millis(self.settings.polling_interval_ms.max(5_000))
    }

    fn run(mut self, cmds: Receiver<Cmd>) {
        loop {
            let mut force = false;
            if self.last_full.is_none() {
                let _ = db::prune_standalone_sessions(&self.conn);
                let _ = db::prune_sessions(&self.conn);
            }
            match cmds.recv_timeout(TICK) {
                Ok(Cmd::Quit) | Err(RecvTimeoutError::Disconnected) => {
                    self.perf.report(Instant::now());
                    return;
                }
                Ok(Cmd::Refresh { force: f }) => force = f,
                Ok(Cmd::RefreshReviews | Cmd::RefreshPeople | Cmd::ReloadReviews) => {}
                Ok(Cmd::Settings(s)) => {
                    // Filters/sort/process filter may have changed: rebuild now.
                    self.settings = *s;
                    self.last_procs = None;
                    self.last_usage = None;
                    if !self.settings.show_daily_tokens {
                        self.usage = DailyUsage::new();
                    }
                    self.data_version = -1;
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
            let started = Instant::now();
            match self.cycle(force) {
                Ok(true) => self.perf.record("collector.cycle", started.elapsed()),
                Ok(false) => {}
                Err(e) => warn!("tui", "Refresh failed: {e}"),
            }
        }
    }

    /// One collector pass; `false` when there was nothing to do.
    fn cycle(&mut self, force: bool) -> rusqlite::Result<bool> {
        // Review-only mode draws no worktree rows: no process scan, git
        // status, context, token usage or stacks; just the worktree list
        // (for `✓ pulled`) and the integrations' review list.
        let dashboard = !self.settings.reviews_mode();
        let full_due = force || self.last_full.is_none_or(|t| t.elapsed() >= self.poll_interval());
        let procs_due = dashboard && self.last_procs.is_none_or(|t| t.elapsed() >= PROCESS_INTERVAL);
        let usage_due = dashboard
            && self.settings.show_daily_tokens
            && (force || self.last_usage.is_none_or(|t| t.elapsed() >= USAGE_INTERVAL));
        let mut ints_changed = false;
        while let Ok(u) = self.integrations.try_recv() {
            self.fields = u.fields;
            self.health = u.health;
            self.reviews = u.reviews;
            self.review_error = u.review_error;
            self.people = u.people;
            self.review_total = u.review_total;
            self.review_hits = u.review_hits;
            ints_changed = true;
        }
        while let Ok(batch) = self.git_done.try_recv() {
            debug!("tui", "git refresh for {} worktree(s) in {:?}", batch.results.len(), batch.elapsed);
            self.perf.record("collector.git", batch.elapsed);
            ints_changed |= self.git.absorb(batch.results);
        }
        let dv = db::data_version(&self.conn)?;
        if !full_due && !procs_due && !usage_due && !ints_changed && dv == self.data_version {
            return Ok(false);
        }
        self.data_version = dv;

        if force || self.last_sync.is_none_or(|t| t.elapsed() >= SYNC_INTERVAL) {
            self.last_sync = Some(Instant::now());
            let started = Instant::now();
            let synced = worktrees::sync_all(&self.conn);
            self.perf.record("collector.sync", started.elapsed());
            if let Ok(results) = synced {
                for (repo, r) in results {
                    match r {
                        Ok(rep) if !rep.added.is_empty() || !rep.removed.is_empty() => {
                            info!("sync", "{}: +{} -{} worktree(s)", repo.name, rep.added.len(), rep.removed.len())
                        }
                        Ok(_) => {}
                        Err(e) => debug!("sync", "{} failed: {e}", repo.name),
                    }
                }
            }
        }
        if usage_due {
            let started = Instant::now();
            self.usage.refresh(&am_core::harness::enabled(&self.settings));
            self.perf.record("collector.usage", started.elapsed());
            self.last_usage = Some(Instant::now());
        }
        let worktrees = db::worktrees(&self.conn)?;
        if procs_due || (full_due && dashboard) {
            let stamp = am_core::harness::plugins_stamp();
            if stamp != self.plugins_stamp {
                self.plugins_stamp = stamp;
                am_core::harness::reload();
                info!("tui", "Harness plugins changed on disk; reloaded ({} registered)", am_core::harness::all().len());
            }
            let started = Instant::now();
            self.procs = self.collect_procs(&worktrees);
            self.perf.record("collector.procscan", started.elapsed());
            self.last_procs = Some(Instant::now());
        }
        let agents = db::agent_states(&self.conn)?;
        let mut sessions = db::open_worktree_sessions(&self.conn)?;

        // Agent activity since last cycle: refresh that row's git and context.
        let moved: HashSet<&str> = worktrees
            .iter()
            .filter(|w| {
                let updated = agents.get(&w.id).map(|a| a.updated_at.as_str());
                updated.is_some() && self.agent_seen.get(&w.id).map(String::as_str) != updated
            })
            .map(|w| w.id.as_str())
            .collect();
        if force {
            self.git.deep_next();
        }
        if dashboard {
            let jobs = self.git.due(&worktrees, full_due, |w| moved.contains(w.id.as_str()), gitinfo::signature);
            if !jobs.is_empty() {
                debug!("tui", "git refresh queued for {} worktree(s)", jobs.len());
                let _ = self.git_jobs.send(jobs);
            }
        }
        let started = Instant::now();
        for w in &worktrees {
            if !dashboard || !(full_due || moved.contains(w.id.as_str()) || !self.ctx.contains_key(&w.id)) {
                continue;
            }
            let root = Path::new(&w.path);
            let usage = agents.get(&w.id).and_then(|a| session_context(a.agent, a.session_id.as_deref(), root));
            self.ctx.insert(w.id.clone(), usage);
            for s in sessions.get(&w.id).into_iter().flatten() {
                let usage = session_context(s.agent, Some(&s.session_id), root);
                self.ctx.insert(session_key(&w.id, &s.session_id), usage);
            }
        }
        self.perf.record("collector.context", started.elapsed());
        for w in &worktrees {
            if let Some(a) = agents.get(&w.id) {
                self.agent_seen.insert(w.id.clone(), a.updated_at.clone());
            }
        }
        if full_due {
            self.last_full = Some(Instant::now());
        }

        let views: Vec<WorktreeView> = worktrees
            .into_iter()
            .map(|w| {
                let g = self.git.get(&w.id).cloned().unwrap_or_default();
                let p = self.procs.get(&w.id).cloned().unwrap_or_default();
                let (pr, linear) =
                    self.fields.get(&(w.repo_id.clone(), w.branch.clone())).cloned().unwrap_or_default();
                let sessions = live_sessions(sessions.remove(&w.id).unwrap_or_default(), &p.agents)
                    .into_iter()
                    .map(|s| SessionView {
                        context: self.ctx.get(&session_key(&w.id, &s.session_id)).cloned().flatten(),
                        agent: s.agent,
                        session_id: s.session_id,
                        status: s.status,
                        last_response: s.last_response,
                        started_at: s.started_at,
                        updated_at: s.updated_at,
                    })
                    .collect();
                WorktreeView {
                    agent: agents.get(&w.id).cloned(),
                    git: g.status,
                    last_commit: g.commit,
                    pr,
                    linear,
                    has_terminal: p.has_terminal,
                    open_ide: p.open_ide,
                    running_processes: p.running,
                    shell_commands: p.matching_shells,
                    context: self.ctx.get(&w.id).cloned().flatten(),
                    sessions,
                    worktree: w,
                }
            })
            .collect();

        // A subagent waiting on a background command has *stopped* as far as
        // hooks go, but its work isn't done: keep it (and the worktree) going.
        let mut subagents = db::subagents(&self.conn, None, PARKED_SUBAGENT_MINUTES)?;
        let shells: HashMap<&str, usize> = self.procs.iter().map(|(id, p)| (id.as_str(), p.claude_shells)).collect();
        let parked = parked_subagents(&subagents, &views, |id| shells.get(id).copied().unwrap_or(0));
        if parked != self.parked {
            info!("tui", "Parked subagents (stopped, background command still running): {:?}", parked);
            self.parked = parked.clone();
        }
        let views = apply_parked(views, &subagents, &parked);
        let now = am_core::timefmt::now_unix();
        subagents.retain_mut(|(_, s)| {
            if parked.contains(&s.agent_id) {
                s.running = true;
            }
            s.running || s.ended_at.as_deref().and_then(am_core::timefmt::parse_sqlite).is_some_and(|t| now - t <= i64::from(RECENT_SUBAGENT_MINUTES) * 60)
        });

        let repos = db::repositories(&self.conn)?;
        if dashboard && (full_due || self.stack_git.len() != repos.len()) {
            let started = Instant::now();
            self.refresh_stack_git(&repos, &worktrees_for_stacks(&views));
            self.perf.record("collector.stacks", started.elapsed());
        }
        let stack = self.infer_stacks(&views);
        let reviews = self.reviews_with_worktrees(&views);
        let mut roots: Vec<String> = views.iter().map(|v| v.worktree.path.clone()).collect();
        // Review-only: every worktree, unfiltered, so a pulled PR's ⏎/`d`
        // always find theirs (the dashboard's hide* filters don't apply).
        let views = if dashboard { grouping::apply_filters(views, &self.settings) } else { views };
        let mut groups = grouping::build_groups(&repos, views, &self.settings);
        if let Some(g) = if dashboard { self.standalone_group()? } else { None } {
            roots.extend(g.worktrees.iter().map(|v| v.worktree.path.clone()));
            groups.push(g);
        }
        let today_tokens = if dashboard { self.today_tokens(&roots, &groups) } else { None };
        let review_error = self.review_error.clone();
        let review_people = people_with_authors(&self.people, reviews.as_deref().unwrap_or_default());
        let snapshot = Snapshot {
            repos,
            groups,
            subagents,
            health: self.health.clone(),
            stack,
            today_tokens,
            reviews,
            review_error,
            review_people,
            review_total: self.review_total,
            review_query_hits: self.review_hits.clone(),
        };
        if self.last_sent.as_ref() != Some(&snapshot) {
            let _ = self.tx.send(snapshot.clone());
            self.last_sent = Some(snapshot);
        }
        Ok(true)
    }
}

/// `people` plus any review author not among them (no name), by login.
fn people_with_authors(people: &[ReviewPerson], reviews: &[ReviewRequest]) -> Vec<ReviewPerson> {
    let mut out = people.to_vec();
    for r in reviews {
        if !r.author.is_empty() && !out.iter().any(|p| p.login.eq_ignore_ascii_case(&r.author)) {
            out.push(ReviewPerson { login: r.author.clone(), name: String::new() });
        }
    }
    out.sort_by_key(|p| p.login.to_lowercase());
    out
}

fn worktrees_for_stacks(views: &[WorktreeView]) -> Vec<Worktree> {
    views.iter().map(|v| v.worktree.clone()).collect()
}

impl Worker {
    /// The review list with each PR's local worktree (same repo and branch),
    /// if one is checked out.
    fn reviews_with_worktrees(&self, views: &[WorktreeView]) -> Option<Vec<ReviewRequest>> {
        let mut list = self.reviews.clone()?;
        for r in &mut list {
            r.worktree_id = views
                .iter()
                .find(|v| v.worktree.repo_id == r.repo_id && v.worktree.branch == r.head_ref_name)
                .map(|v| v.worktree.id.clone());
        }
        // Listed only to remind you to remove the worktree: gone with it.
        list.retain(|r| !r.approved_only || r.worktree_id.is_some());
        Some(list)
    }

    /// Today's tokens: everything, or (`dailyTokensScope: "visible"`) only
    /// the usage attributed to a row on screen. Each session's cwd goes to
    /// the most specific of *all* known roots, so a visible main checkout
    /// doesn't pick up its hidden nested worktrees.
    fn today_tokens(&self, roots: &[String], groups: &[am_core::model::WorktreeGroup]) -> Option<am_core::usage::Tally> {
        if !self.settings.show_daily_tokens || self.last_usage.is_none() {
            return None;
        }
        if !self.settings.daily_tokens_visible_only() {
            return Some(self.usage.total());
        }
        let roots: Vec<&str> = roots.iter().map(String::as_str).collect();
        let by_root = self.usage.by_root(&roots);
        let visible: HashSet<&str> = groups.iter().flat_map(|g| &g.worktrees).map(|v| v.worktree.path.as_str()).collect();
        let mut tally = am_core::usage::Tally::default();
        by_root.iter().filter(|(r, _)| visible.contains(r.as_str())).for_each(|(_, t)| tally.add(t));
        Some(tally)
    }

    fn refresh_stack_git(&mut self, repos: &[am_core::model::Repository], worktrees: &[Worktree]) {
        for repo in repos {
            let path = Path::new(&repo.path);
            let tips = stacks::branch_tips(path);
            let mut sig: Vec<String> = tips.iter().map(|(b, s)| format!("{b}={s}")).collect();
            sig.sort();
            let sig = sig.join(",");
            if self.stack_git.get(&repo.id).is_some_and(|(s, _)| *s == sig) {
                continue;
            }
            let branches = stack_branches(worktrees, &repo.id);
            let main = worktrees::main_branch(path).unwrap_or_else(|_| "main".into());
            let trunk = stacks::trunk_ref(path, &main);
            let started = Instant::now();
            let view = stacks::collect(path, &branches, &trunk, tips);
            debug!("stacks", "{}: rev-lists for {} branch(es) in {:?}", repo.name, view.unmerged.len(), started.elapsed());
            self.stack_git.insert(repo.id.clone(), (sig, view));
        }
    }

    /// child worktree id → parent worktree id, across all repos.
    fn infer_stacks(&self, views: &[WorktreeView]) -> HashMap<String, String> {
        let worktrees = worktrees_for_stacks(views);
        let pr_base: HashMap<String, String> = views
            .iter()
            .filter_map(|v| {
                let (pr, _) = self.fields.get(&(v.worktree.repo_id.clone(), v.worktree.branch.clone()))?;
                let pr = pr.as_ref().filter(|p| p.state == "OPEN")?;
                Some((v.worktree.id.clone(), pr.base_ref_name.clone()?))
            })
            .collect();
        let mut out = HashMap::new();
        for (repo_id, (_, git)) in &self.stack_git {
            out.extend(stacks::infer_parents(&stack_branches(&worktrees, repo_id), git, &pr_base));
        }
        out
    }

    /// "Other sessions": open standalone sessions as pseudo-worktree rows so
    /// selection, Enter and `d` work on them like on any row.
    fn standalone_group(&mut self) -> rusqlite::Result<Option<am_core::model::WorktreeGroup>> {
        use am_core::model::{Repository, STANDALONE_REPO_ID, WorktreeGroup};
        let home = am_core::paths::home_dir().to_string_lossy().into_owned();
        let mut sessions = db::standalone_sessions(&self.conn)?;
        // A session whose directory is gone (a deleted worktree whose agent
        // never sent SessionEnd) can never update again: forget it rather
        // than show it as running forever.
        sessions.retain(|(_, path, a)| {
            if Path::new(path).exists() {
                return true;
            }
            info!("collector", "Removed standalone session at {path} ({}): directory no longer exists", a.status.as_str());
            let _ = db::remove_standalone_session(&self.conn, path);
            false
        });
        let views: Vec<WorktreeView> = sessions
            .into_iter()
            .filter(|(_, _, a)| grouping::is_open_now(Some(a)))
            .map(|(id, path, agent)| {
                let key = format!("standalone:{id}");
                // Re-read the transcript only when the session moved.
                if self.agent_seen.get(&key) != Some(&agent.updated_at) {
                    let usage = session_context(agent.agent, agent.session_id.as_deref(), Path::new(&path));
                    self.ctx.insert(key.clone(), usage);
                    self.agent_seen.insert(key.clone(), agent.updated_at.clone());
                }
                let context = self.ctx.get(&key).cloned().flatten();
                let short = path.strip_prefix(&home).map(|p| format!("~{p}")).unwrap_or_else(|| path.clone());
                WorktreeView {
                    worktree: Worktree {
                        id: key,
                        repo_id: STANDALONE_REPO_ID.into(),
                        path,
                        branch: short.clone(),
                        name: short,
                        custom_name: None,
                        is_main: false,
                        created_at: agent.updated_at.clone(),
                        tmp_space: false,
                    },
                    agent: Some(agent),
                    git: None,
                    last_commit: None,
                    pr: None,
                    linear: None,
                    has_terminal: false,
                    open_ide: None,
                    running_processes: Vec::new(),
                    shell_commands: 0,
                    context,
                    sessions: Vec::new(),
                }
            })
            .collect();
        Ok((!views.is_empty()).then(|| WorktreeGroup {
            repo: Repository { id: STANDALONE_REPO_ID.into(), path: String::new(), name: "Other sessions".into() },
            worktrees: views,
        }))
    }

    fn collect_procs(&self, worktrees: &[Worktree]) -> HashMap<String, ProcData> {
        let snap = procscan::capture();
        let roots: Vec<PathBuf> = worktrees.iter().map(|w| PathBuf::from(&w.path)).collect();
        let mut agents = snap.agent_sessions(&roots);
        let mut running = if self.settings.show_running_processes {
            snap.running_processes(&roots, &self.settings.running_process_filter)
        } else {
            HashMap::new()
        };
        worktrees
            .iter()
            .map(|w| {
                let root = PathBuf::from(&w.path);
                let data = ProcData {
                    has_terminal: snap.has_terminal(&root),
                    open_ide: snap.open_ide(&root),
                    running: running.remove(&root).unwrap_or_default(),
                    claude_shells: snap.claude_shell_commands(&root),
                    matching_shells: snap.claude_shell_commands_matching(&root, &self.settings.running_process_filter),
                    agents: agents.remove(&root).unwrap_or_default(),
                };
                (w.id.clone(), data)
            })
            .collect()
    }
}

/// Opens a long-lived thread's connection. If that fails, retry every second
/// instead of returning: a thread that gave up left the dashboard empty and
/// the review page on "Loading…" for the rest of the run. Settings changes
/// sent meanwhile are kept; refresh requests can go, since each thread starts
/// with a full pass. `None` when the TUI quits first.
fn open_db(
    module: &str,
    cmds: &Receiver<Cmd>,
    settings: &mut Settings,
    open: impl Fn() -> rusqlite::Result<Connection>,
) -> Option<Connection> {
    let mut failing = false;
    loop {
        match open() {
            Ok(conn) => {
                if failing {
                    info!(module, "Database opened after retrying");
                }
                return Some(conn);
            }
            Err(e) if !failing => {
                warn!(module, "Cannot open database: {e}; retrying every second");
                failing = true;
            }
            Err(e) => debug!(module, "Still cannot open database: {e}"),
        }
        match cmds.recv_timeout(Duration::from_secs(1)) {
            Ok(Cmd::Quit) | Err(RecvTimeoutError::Disconnected) => return None,
            Ok(Cmd::Settings(s)) => *settings = *s,
            Ok(_) | Err(RecvTimeoutError::Timeout) => {}
        }
    }
}

/// Owns GitHub/Linear fetching. `gh` can take seconds, so it lives on its own
/// thread and hands the collector a ready-made field map. Cached values are
/// sent before the first fetch, so PR badges and tickets appear immediately
/// (and survive being offline).
fn run_integrations(mut settings: Settings, cmds: Receiver<Cmd>, tx: Sender<IntegrationUpdate>) {
    let Some(conn) = open_db("integrations", &cmds, &mut settings, db::open) else { return };
    let mut ints = Integrations::new();
    let mut perf = Perf::new();
    ints.hydrate(&conn);
    let (mut last_gh, mut last_linear): (Option<Instant>, Option<Instant>) = (None, None);
    let mut last_reviews: Option<Instant> = None;
    let mut known_repos: Vec<String> = Vec::new();
    let mut known_branches: Vec<(String, String)> = Vec::new();
    let mut first = true;

    loop {
        let mut force = false;
        let mut reviews_now = false;
        let mut people_now = false;
        match cmds.recv_timeout(Duration::from_secs(1)) {
            Ok(Cmd::Quit) | Err(RecvTimeoutError::Disconnected) => {
                perf.report(Instant::now());
                return;
            }
            Ok(Cmd::Refresh { force: f }) => force = f,
            Ok(Cmd::RefreshReviews) => reviews_now = true,
            Ok(Cmd::RefreshPeople) => people_now = true,
            Ok(Cmd::ReloadReviews) => {
                ints.forget_reviews(&conn);
                // Show "Loading…" now rather than the old list until GitHub answers.
                let _ = tx.send(update(&ints, &known_branches, &settings));
                reviews_now = true;
                people_now = true;
            }
            Ok(Cmd::Settings(s)) => {
                if settings.linear_auto_nickname
                    && !s.linear_auto_nickname
                    && let Ok(n) = db::clear_linear_nicknames(&conn)
                {
                    info!("integrations", "Cleared {n} Linear auto-nickname(s)");
                }
                // A newly enabled source or new API key should fetch right away.
                let enabled_now = (s.gh_pr_status && !settings.gh_pr_status)
                    || (s.linear_enabled && !settings.linear_enabled)
                    || s.linear_api_key != settings.linear_api_key;
                if settings.review_requests_on() && !s.review_requests_on() {
                    ints.clear_reviews(&conn);
                    info!("integrations", "Review requests off; cleared the list");
                }
                // Any change to what the search asks for re-runs it now.
                let reviews_changed = s.review_requests_on() != settings.review_requests_on()
                    || s.review_requests_scope != settings.review_requests_scope
                    || group_queries(&s) != group_queries(&settings)
                    || (wants_total(&s) && !wants_total(&settings));
                settings = *s;
                if enabled_now {
                    last_gh = None;
                    last_linear = None;
                }
                if enabled_now || reviews_changed {
                    last_reviews = None;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
        let gh_every = Duration::from_millis(settings.gh_polling_interval_ms.max(30_000));
        let linear_every = Duration::from_millis(settings.linear_polling_interval_ms.max(30_000));
        let linear_on = settings.linear_enabled && !settings.linear_api_key.is_empty();
        let Ok(repos) = db::repositories(&conn) else { continue };
        let Ok(worktrees) = db::worktrees(&conn) else { continue };
        let groups: Vec<RepoBranches> = repos
            .iter()
            .map(|r| {
                let branches = worktrees.iter().filter(|w| w.repo_id == r.id).map(|w| w.branch.clone()).collect();
                RepoBranches::new(r.id.clone(), r.path.clone(), branches)
            })
            .collect();
        let mut branches: Vec<(String, String)> = worktrees.iter().map(|w| (w.repo_id.clone(), w.branch.clone())).collect();
        branches.sort();
        // A new worktree should get its PR/ticket now, not in three minutes.
        let new_branches = !first && branches != known_branches;

        let mut repo_ids: Vec<String> = repos.iter().map(|r| r.id.clone()).collect();
        repo_ids.sort();
        let reviews_every = Duration::from_millis(settings.review_polling_interval_ms.max(30_000));
        let reviews_due = settings.review_requests_on()
            && (reviews_now
                || (force && settings.gh_refresh_on_manual)
                || repo_ids != known_repos
                || last_reviews.is_none_or(|t| t.elapsed() >= reviews_every));

        // Review-only mode shows no worktree rows, so their PRs and tickets
        // aren't fetched (review rows get theirs with the review search).
        let dashboard = !settings.reviews_mode();
        let gh_due = dashboard
            && settings.gh_pr_status
            && ((force && settings.gh_refresh_on_manual) || new_branches || last_gh.is_none_or(|t| t.elapsed() >= gh_every));
        let linear_due = dashboard
            && linear_on
            && ((force && settings.linear_refresh_on_manual) || new_branches || last_linear.is_none_or(|t| t.elapsed() >= linear_every));

        let people_due = people_now && settings.gh_pr_status;
        if !(gh_due || linear_due || reviews_due || people_due || first || new_branches) {
            continue;
        }
        // Intervals run from the start of a cycle, so a slow GitHub PR fetch
        // ahead of the review search doesn't push every refresh later.
        let cycle_start = Instant::now();
        if first {
            // Publish what's cached before the (slow) first fetch.
            let _ = tx.send(update(&ints, &branches, &settings));
            first = false;
        }
        if gh_due {
            let started = Instant::now();
            let outcome = ints.refresh_github(&conn, &groups, force);
            info!("integrations", "github refresh: {outcome:?} in {:?}", started.elapsed());
            perf.record("integrations.github", started.elapsed());
            last_gh = Some(Instant::now());
        }
        if linear_due {
            let started = Instant::now();
            let outcome = ints.refresh_linear(&conn, &settings.linear_api_key, &groups, force);
            info!("integrations", "linear refresh: {outcome:?} in {:?}", started.elapsed());
            perf.record("integrations.linear", started.elapsed());
            last_linear = Some(Instant::now());
        }
        if reviews_due {
            let started = Instant::now();
            let review_repos: Vec<ReviewRepo> = repos
                .iter()
                .map(|r| {
                    let pulled =
                        worktrees.iter().filter(|w| w.repo_id == r.id && w.tmp_space).map(|w| w.branch.clone()).collect();
                    ReviewRepo::new(r.id.clone(), r.name.clone(), r.path.clone()).with_pulled(pulled)
                })
                .collect();
            let outcome = ints.refresh_reviews(&conn, &review_repos, &settings, force || reviews_now);
            info!("integrations", "review requests refresh: {outcome:?} in {:?}", started.elapsed());
            perf.record("integrations.reviews", started.elapsed());
            last_reviews = Some(cycle_start);
            known_repos = repo_ids;
        }
        if people_due {
            let review_repos: Vec<ReviewRepo> =
                repos.iter().map(|r| ReviewRepo::new(r.id.clone(), r.name.clone(), r.path.clone())).collect();
            ints.refresh_people(&conn, &review_repos);
        }
        known_branches = branches.clone();
        if dashboard && settings.linear_enabled && settings.linear_auto_nickname {
            auto_nickname(&conn, &ints, &worktrees);
        }
        let _ = tx.send(update(&ints, &branches, &settings));
    }
}

/// Port of `autoSetLinearNicknames`: name un-named worktrees after their
/// Linear ticket title (user-set names are never touched).
fn auto_nickname(conn: &Connection, ints: &Integrations, worktrees: &[Worktree]) {
    for w in worktrees.iter().filter(|w| w.custom_name.is_none()) {
        if let (_, Some(l)) = ints.fields_for(&w.repo_id, &w.branch) {
            info!("integrations", "Auto-setting nickname for {} from Linear: {:?}", w.branch, l.title);
            let _ = db::update_worktree_custom_name(conn, &w.id, Some(&l.title), Some("linear"));
        }
    }
}

fn update(ints: &Integrations, branches: &[(String, String)], settings: &Settings) -> IntegrationUpdate {
    let fields = branches.iter().map(|(r, b)| ((r.clone(), b.clone()), ints.fields_for(r, b))).collect();
    let h = ints.health();
    let on = settings.review_requests_on();
    let reviews = on.then(|| ints.reviews().map(<[_]>::to_vec)).flatten();
    let review_error = on.then(|| ints.review_error().cloned()).flatten();
    let github_rate_limited = h.github_failing
        && h.last_github_error.as_deref().is_some_and(am_core::integrations::breaker::is_rate_limit_error);
    IntegrationUpdate {
        fields,
        health: Health { github_failing: h.github_failing, linear_failing: h.linear_failing, github_rate_limited },
        reviews,
        review_error,
        people: if settings.gh_pr_status { ints.people().to_vec() } else { Vec::new() },
        review_total: (on && wants_total(settings)).then(|| ints.review_total()).flatten(),
        review_hits: if on { ints.review_hits().clone() } else { Default::default() },
    }
}

/// The review groups' GitHub search terms, which the search runs.
fn group_queries(settings: &Settings) -> Vec<String> {
    am_core::review_groups::queries(&settings.review_groups, &settings.review_default_group)
}

/// The review badge shows the unfiltered count, which the search then fetches.
fn wants_total(settings: &Settings) -> bool {
    settings.review_badge_total
}

fn stack_branches(worktrees: &[Worktree], repo_id: &str) -> Vec<stacks::Branch> {
    worktrees
        .iter()
        .filter(|w| w.repo_id == repo_id)
        .map(|w| stacks::Branch { worktree_id: w.id.clone(), name: w.branch.clone(), is_main: w.is_main })
        .collect()
}

/// Subagents that stopped without handing back a result while a Claude Code
/// shell command is still running in their worktree: they're parked waiting
/// on a background command and will be woken when it finishes. Only the
/// worktree's current session counts. A finished background subagent calls
/// `SubagentHandback`; one that just replied (no handback) has nothing left
/// running, so it isn't caught by the shell check. Only the worktree's live
/// sessions (its headline and any listed session) count.
fn parked_subagents(
    subagents: &[(Option<String>, Subagent)],
    views: &[WorktreeView],
    claude_shells: impl Fn(&str) -> usize,
) -> HashSet<String> {
    let live: HashSet<(&str, &str)> = views
        .iter()
        .flat_map(|v| {
            let wt = v.worktree.id.as_str();
            let head = v.agent.as_ref().and_then(|a| a.session_id.as_deref());
            head.into_iter().chain(v.sessions.iter().map(|s| s.session_id.as_str())).map(move |sid| (wt, sid))
        })
        .collect();
    subagents
        .iter()
        .filter(|(wt, s)| {
            let Some(wt) = wt.as_deref() else { return false };
            !s.running
                && s.last_tool.as_deref() != Some(HANDBACK_TOOL)
                && live.contains(&(wt, s.session_id.as_str()))
                && claude_shells(wt) > 0
        })
        .map(|(_, s)| s.agent_id.clone())
        .collect()
}

/// A worktree (or one of its sessions) whose main turn is `done` but has a
/// parked subagent is still `delegating`. (Display only; the DB keeps the
/// hook-driven status.)
fn apply_parked(mut views: Vec<WorktreeView>, subagents: &[(Option<String>, Subagent)], parked: &HashSet<String>) -> Vec<WorktreeView> {
    if parked.is_empty() {
        return views;
    }
    let busy: HashSet<(&str, &str)> = subagents
        .iter()
        .filter(|(_, s)| parked.contains(&s.agent_id))
        .filter_map(|(wt, s)| Some((wt.as_deref()?, s.session_id.as_str())))
        .collect();
    for v in &mut views {
        let wt = v.worktree.id.as_str();
        for s in &mut v.sessions {
            if s.status == Status::Done && busy.contains(&(wt, s.session_id.as_str())) {
                s.status = Status::Delegating;
            }
        }
        if busy.iter().any(|(w, _)| *w == wt)
            && let Some(a) = v.agent.as_mut()
            && a.status == Status::Done
        {
            debug!("tui", "{} shown as delegating: parked subagent", v.worktree.branch);
            a.status = Status::Delegating;
        }
    }
    views
}

/// Context usage of one session, read by the session's own harness plugin
/// (Claude's only, so far). Worktree, session and standalone rows all go
/// through here, so a Gemini session is never looked up as a Claude transcript.
fn session_context(harness: HarnessKind, session_id: Option<&str>, root: &Path) -> Option<ContextUsage> {
    harness.provider().context_usage(root, session_id?)
}

/// `ctx` key of one session's context usage.
fn session_key(worktree_id: &str, session_id: &str) -> String {
    format!("{worktree_id}\u{1f}{session_id}")
}

/// The open sessions of a worktree that are really running, oldest first.
/// A session killed without `SessionEnd` stays open in the DB, so when the
/// process scan sees fewer CLIs of a harness than it has open sessions, only
/// the most recently active ones are kept. No process at all (an IDE
/// extension, an unreadable cwd) proves nothing: those are all kept.
fn live_sessions(mut rows: Vec<db::SessionRow>, processes: &HashMap<&'static str, usize>) -> Vec<db::SessionRow> {
    rows.retain(|s| grouping::session_is_live(s.is_open, s.status, &s.updated_at));
    let mut keep = vec![true; rows.len()];
    for (harness, &n) in processes {
        let mut of: Vec<usize> = (0..rows.len()).filter(|&i| rows[i].agent.id() == *harness).collect();
        if n == 0 || of.len() <= n {
            continue;
        }
        of.sort_by(|&a, &b| rows[b].updated_at.cmp(&rows[a].updated_at));
        for &i in &of[n..] {
            keep[i] = false;
        }
    }
    let mut k = keep.into_iter();
    rows.retain(|_| k.next().unwrap_or(true));
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use am_core::model::AgentState;

    /// An opener that fails `failures` times, then opens an in-memory DB.
    fn flaky_open(failures: usize) -> impl Fn() -> rusqlite::Result<Connection> {
        let calls = std::cell::Cell::new(0);
        move || {
            calls.set(calls.get() + 1);
            if calls.get() <= failures {
                Err(rusqlite::Error::InvalidQuery)
            } else {
                Connection::open_in_memory()
            }
        }
    }

    #[test]
    fn a_thread_that_cannot_open_the_db_retries_and_keeps_new_settings() {
        let (cmd, cmds) = mpsc::channel();
        let mut settings = Settings::default();
        let changed = Settings { review_requests_enabled: !settings.review_requests_enabled, ..Settings::default() };
        // Each queued command ends one wait, so the retries don't sleep.
        cmd.send(Cmd::Settings(Box::new(changed.clone()))).unwrap();
        cmd.send(Cmd::RefreshReviews).unwrap();
        assert!(open_db("test", &cmds, &mut settings, flaky_open(2)).is_some());
        assert_eq!(settings.review_requests_enabled, changed.review_requests_enabled);
    }

    #[test]
    fn a_thread_that_cannot_open_the_db_still_quits() {
        let (cmd, cmds) = mpsc::channel();
        cmd.send(Cmd::Quit).unwrap();
        assert!(open_db("test", &cmds, &mut Settings::default(), flaky_open(usize::MAX)).is_none());
        let (cmd, cmds) = mpsc::channel::<Cmd>();
        drop(cmd);
        assert!(open_db("test", &cmds, &mut Settings::default(), flaky_open(usize::MAX)).is_none());
    }

    fn view(id: &str, session: &str, status: Status) -> WorktreeView {
        WorktreeView {
            worktree: Worktree {
                id: id.into(),
                repo_id: "r".into(),
                path: format!("/r/{id}"),
                branch: id.into(),
                name: id.into(),
                custom_name: None,
                is_main: false,
                created_at: "2026-10-01 00:00:00".into(),
                tmp_space: false,
            },
            agent: Some(AgentState {
                status,
                session_id: Some(session.into()),
                last_response: None,
                is_open: true,
                active_subagents: 0,
                updated_at: "2026-10-11 21:00:00".into(),
                agent: Default::default(),
            }),
            git: None,
            last_commit: None,
            pr: None,
            linear: None,
            has_terminal: false,
            open_ide: None,
            running_processes: Vec::new(),
            shell_commands: 0,
            context: None,
            sessions: Vec::new(),
        }
    }

    fn sub(wt: &str, session: &str, id: &str, running: bool, last_tool: &str) -> (Option<String>, Subagent) {
        (
            Some(wt.into()),
            Subagent {
                session_id: session.into(),
                agent_id: id.into(),
                agent_type: Some("general-purpose".into()),
                description: None,
                running,
                tool_count: 1,
                last_tool: Some(last_tool.into()),
                last_tool_input: None,
                started_at: "2026-10-11 21:00:00".into(),
                ended_at: (!running).then(|| "2026-10-11 21:05:00".into()),
                updated_at: "2026-10-11 21:05:00".into(),
                transcript_path: None,
            },
        )
    }

    /// Review-only mode lists worktrees (for `✓ pulled`) but runs none of
    /// the dashboard's per-row work: no git status, process scan, context,
    /// token usage or stacks.
    #[test]
    fn review_only_mode_does_no_dashboard_work() {
        let dir = tempfile::tempdir().unwrap();
        let conn = db::open_at(&dir.path().join("am.db")).unwrap();
        let repo = db::add_repository(&conn, &dir.path().display().to_string(), "work").unwrap();
        let wt = dir.path().join("wt");
        db::upsert_worktree(&conn, &repo.id, &wt.display().to_string(), "feature/x", "x", false).unwrap();
        let (tx, rx) = mpsc::channel();
        let run = |mode: &str| {
            let conn = db::open_at(&dir.path().join("am.db")).unwrap();
            let settings = Settings { app_mode: mode.into(), ..Settings::default() };
            let mut w = Worker::new(settings, conn, tx.clone(), mpsc::channel().1);
            w.cycle(true).unwrap();
            w
        };
        let w = run("reviews");
        assert_eq!(w.git.in_flight(), 0, "no git status jobs");
        assert!(w.last_procs.is_none() && w.last_usage.is_none(), "no process scan or token usage");
        assert!(w.ctx.is_empty() && w.stack_git.is_empty(), "no context or stacks");
        let snap = rx.try_iter().last().expect("a snapshot");
        assert_eq!(snap.groups.iter().map(|g| g.worktrees.len()).sum::<usize>(), 1, "the worktree is still listed");
        // The full dashboard does all of it.
        let w = run("full");
        assert_eq!(w.git.in_flight(), 1);
        assert!(w.last_procs.is_some() && !w.ctx.is_empty());
    }

    #[test]
    fn an_approved_only_pr_goes_with_its_worktree() {
        let dir = tempfile::tempdir().unwrap();
        let conn = db::open_at(&dir.path().join("am.db")).unwrap();
        let mut w = Worker::new(Settings::default(), conn, mpsc::channel().0, mpsc::channel().1);
        let pr = |branch: &str, approved_only: bool| {
            let mut r = crate::reviews_ui::tests::review("r", 1, "2026-10-01T00:00:00Z");
            r.head_ref_name = branch.into();
            r.approved_only = approved_only;
            r
        };
        w.reviews = Some(vec![pr("w1", true), pr("gone", true), pr("asked", false)]);
        let list = w.reviews_with_worktrees(&[view("w1", "s1", Status::Done)]).unwrap();
        let heads: Vec<&str> = list.iter().map(|r| r.head_ref_name.as_str()).collect();
        assert_eq!(heads, ["w1", "asked"]);
        assert_eq!(list[0].worktree_id.as_deref(), Some("w1"));
    }

    #[test]
    fn a_stopped_subagent_with_a_background_command_is_parked() {
        let views = vec![view("w1", "s1", Status::Done), view("w2", "s2", Status::Done)];
        let subs = vec![
            sub("w1", "s1", "parked", false, "Bash"),
            sub("w1", "s1", "finished", false, HANDBACK_TOOL),
            sub("w1", "s0", "old-session", false, "Bash"),
            sub("w1", "s1", "live", true, "Bash"),
            // Stopped without a handback, but nothing is running in w2.
            sub("w2", "s2", "replied", false, "Read"),
        ];
        let shells = |wt: &str| usize::from(wt == "w1");
        let parked = parked_subagents(&subs, &views, shells);
        assert_eq!(parked, HashSet::from(["parked".to_string()]));

        let views = apply_parked(views, &subs, &parked);
        assert_eq!(views[0].status(), Some(Status::Delegating), "done + parked subagent → delegating");
        assert_eq!(views[1].status(), Some(Status::Done));
    }

    #[test]
    fn context_is_read_by_the_sessions_own_harness() {
        // Gemini has no context reader: no Claude transcript lookup, no bar.
        assert_eq!(session_context(HarnessKind::GEMINI, Some("s1"), Path::new("/r/w1")), None);
        assert_eq!(session_context(HarnessKind::CLAUDE, None, Path::new("/r/w1")), None, "no session, nothing to read");
    }

    fn row(harness: HarnessKind, id: &str, status: Status, updated: &str) -> db::SessionRow {
        db::SessionRow {
            agent: harness,
            session_id: id.into(),
            status,
            last_response: None,
            is_open: true,
            started_at: "2099-01-01 00:00:00".into(),
            updated_at: updated.into(),
        }
    }

    #[test]
    fn sessions_beyond_the_running_processes_are_dropped_newest_kept() {
        let c = HarnessKind::CLAUDE;
        // Far-future timestamps so the idle-grace rule never applies.
        let rows = vec![
            row(c, "old", Status::Done, "2099-01-01 00:01:00"),
            row(c, "mid", Status::Executing, "2099-01-01 00:03:00"),
            row(HarnessKind::GEMINI, "gem", Status::Done, "2099-01-01 00:00:30"),
            row(c, "new", Status::Waiting, "2099-01-01 00:05:00"),
        ];
        let ids = |v: Vec<db::SessionRow>| v.into_iter().map(|s| s.session_id).collect::<Vec<_>>();
        // Two claude processes: the stale "old" one goes; order is kept.
        let procs = HashMap::from([("claude", 2)]);
        assert_eq!(ids(live_sessions(rows.clone(), &procs)), ["mid", "gem", "new"]);
        // No process seen proves nothing.
        assert_eq!(ids(live_sessions(rows.clone(), &HashMap::new())), ["old", "mid", "gem", "new"]);
        // Closed sessions and long-idle ones aren't live.
        let mut closed = rows.clone();
        closed[1].is_open = false;
        closed[0].status = Status::Idle;
        closed[0].updated_at = "2000-01-01 00:00:00".into();
        assert_eq!(ids(live_sessions(closed, &HashMap::new())), ["gem", "new"]);
    }

    #[test]
    fn only_done_is_promoted() {
        let subs = vec![sub("w1", "s1", "parked", false, "Bash")];
        let parked = HashSet::from(["parked".to_string()]);
        for s in [Status::Executing, Status::Waiting, Status::Idle] {
            let views = apply_parked(vec![view("w1", "s1", s)], &subs, &parked);
            assert_eq!(views[0].status(), Some(s), "{s:?} is left alone");
        }
    }
}
