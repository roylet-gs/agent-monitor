//! Background collector: builds `Snapshot`s off the UI thread.
//!
//! Change detection is `PRAGMA data_version` on a long-lived connection —
//! it bumps whenever any *other* connection (a hook process) commits, so the
//! dashboard reacts to agent events within ~200 ms without owning the pub/sub
//! socket (which the TS TUI may hold during the migration) and without
//! rescanning git. Git is refreshed for the worktrees whose agent just did
//! something, plus a full sweep every polling interval.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use am_core::integrations::{Integrations, RepoBranches};
use am_core::model::{CommitInfo, GitStatus, Ide, LinearInfo, PrInfo, RunningProcess, Worktree, WorktreeView};
use am_core::settings::Settings;
use am_core::{db, debug, gitinfo, grouping, info, procscan, warn, worktrees};
use rusqlite::Connection;

use crate::app::{Health, Snapshot};

const TICK: Duration = Duration::from_millis(200);
/// A native process snapshot costs ~2 ms, so terminals/editors/dev servers
/// are re-checked often, independent of the (slower) git sweep.
const PROCESS_INTERVAL: Duration = Duration::from_secs(2);
/// Pick up worktrees created/removed outside am (`git worktree add`, other
/// tools): one `git worktree list` per repo.
const SYNC_INTERVAL: Duration = Duration::from_secs(10);
/// Recently finished subagents stay visible this long.
const RECENT_SUBAGENT_MINUTES: u32 = 30;

enum Cmd {
    Refresh { force: bool },
    Settings(Box<Settings>),
    Quit,
}

type Fields = HashMap<(String, String), (Option<PrInfo>, Option<LinearInfo>)>;

/// Sent by the integrations thread after hydrating and after every refresh.
struct IntegrationUpdate {
    fields: Fields,
    health: Health,
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
                let conn = match db::open() {
                    Ok(c) => c,
                    Err(e) => {
                        warn!("tui", "Cannot open database: {e}");
                        return;
                    }
                };
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
}

#[derive(Default, Clone)]
struct GitData {
    status: Option<GitStatus>,
    commit: Option<CommitInfo>,
}

struct Worker {
    settings: Settings,
    conn: Connection,
    tx: Sender<Snapshot>,
    integrations: Receiver<IntegrationUpdate>,
    fields: Fields,
    health: Health,
    git: HashMap<String, GitData>,
    procs: HashMap<String, ProcData>,
    last_procs: Option<Instant>,
    last_sync: Option<Instant>,
    agent_seen: HashMap<String, String>,
    last_full: Option<Instant>,
    data_version: i64,
    last_sent: Option<Snapshot>,
}

impl Worker {
    fn new(settings: Settings, conn: Connection, tx: Sender<Snapshot>, integrations: Receiver<IntegrationUpdate>) -> Worker {
        Worker {
            settings,
            conn,
            tx,
            integrations,
            fields: HashMap::new(),
            health: Health::default(),
            git: HashMap::new(),
            procs: HashMap::new(),
            last_procs: None,
            last_sync: None,
            agent_seen: HashMap::new(),
            last_full: None,
            data_version: -1,
            last_sent: None,
        }
    }

    fn poll_interval(&self) -> Duration {
        Duration::from_millis(self.settings.polling_interval_ms.max(5_000))
    }

    fn run(mut self, cmds: Receiver<Cmd>) {
        loop {
            let mut force = false;
            match cmds.recv_timeout(TICK) {
                Ok(Cmd::Quit) | Err(RecvTimeoutError::Disconnected) => return,
                Ok(Cmd::Refresh { force: f }) => force = f,
                Ok(Cmd::Settings(s)) => {
                    // Filters/sort/process filter may have changed: rebuild now.
                    self.settings = *s;
                    self.last_procs = None;
                    self.data_version = -1;
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
            if let Err(e) = self.cycle(force) {
                warn!("tui", "Refresh failed: {e}");
            }
        }
    }

    fn cycle(&mut self, force: bool) -> rusqlite::Result<()> {
        let full_due = force || self.last_full.is_none_or(|t| t.elapsed() >= self.poll_interval());
        let procs_due = self.last_procs.is_none_or(|t| t.elapsed() >= PROCESS_INTERVAL);
        let mut ints_changed = false;
        while let Ok(u) = self.integrations.try_recv() {
            self.fields = u.fields;
            self.health = u.health;
            ints_changed = true;
        }
        let dv = db::data_version(&self.conn)?;
        if !full_due && !procs_due && !ints_changed && dv == self.data_version {
            return Ok(());
        }
        self.data_version = dv;

        if force || self.last_sync.is_none_or(|t| t.elapsed() >= SYNC_INTERVAL) {
            self.last_sync = Some(Instant::now());
            if let Ok(results) = worktrees::sync_all(&self.conn) {
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
        let worktrees = db::worktrees(&self.conn)?;
        if procs_due || full_due {
            self.procs = self.collect_procs(&worktrees);
            self.last_procs = Some(Instant::now());
        }
        let agents = db::agent_states(&self.conn)?;

        // Which worktrees need fresh git data this cycle?
        let stale: Vec<&Worktree> = if full_due {
            worktrees.iter().collect()
        } else {
            worktrees
                .iter()
                .filter(|w| {
                    // New rows (just created or synced) get git data right away.
                    if !self.git.contains_key(&w.id) {
                        return true;
                    }
                    let updated = agents.get(&w.id).map(|a| a.updated_at.as_str());
                    updated.is_some() && self.agent_seen.get(&w.id).map(String::as_str) != updated
                })
                .collect()
        };
        for w in &worktrees {
            if let Some(a) = agents.get(&w.id) {
                self.agent_seen.insert(w.id.clone(), a.updated_at.clone());
            }
        }
        if !stale.is_empty() {
            let started = Instant::now();
            let fresh = collect_git(&stale);
            debug!("tui", "git refresh for {} worktree(s) in {:?}", fresh.len(), started.elapsed());
            self.git.extend(fresh);
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
                WorktreeView {
                    agent: agents.get(&w.id).cloned(),
                    git: g.status,
                    last_commit: g.commit,
                    pr,
                    linear,
                    has_terminal: p.has_terminal,
                    open_ide: p.open_ide,
                    running_processes: p.running,
                    worktree: w,
                }
            })
            .collect();

        let repos = db::repositories(&self.conn)?;
        let groups = grouping::build_groups(&repos, grouping::apply_filters(views, &self.settings), &self.settings);

        let subagents = db::subagents(&self.conn, None, RECENT_SUBAGENT_MINUTES)?;
        let snapshot = Snapshot { repos, groups, subagents, health: self.health.clone() };
        if self.last_sent.as_ref() != Some(&snapshot) {
            let _ = self.tx.send(snapshot.clone());
            self.last_sent = Some(snapshot);
        }
        Ok(())
    }
}

impl Worker {
    fn collect_procs(&self, worktrees: &[Worktree]) -> HashMap<String, ProcData> {
        let snap = procscan::capture();
        let roots: Vec<PathBuf> = worktrees.iter().map(|w| PathBuf::from(&w.path)).collect();
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
                };
                (w.id.clone(), data)
            })
            .collect()
    }
}

/// Two git spawns per worktree, run concurrently.
fn collect_git(worktrees: &[&Worktree]) -> Vec<(String, GitData)> {
    std::thread::scope(|s| {
        let handles: Vec<_> = worktrees
            .iter()
            .map(|w| {
                s.spawn(move || {
                    let p = Path::new(&w.path);
                    (w.id.clone(), GitData { status: gitinfo::status(p), commit: gitinfo::last_commit(p) })
                })
            })
            .collect();
        handles.into_iter().filter_map(|h| h.join().ok()).collect()
    })
}

/// Owns GitHub/Linear fetching. `gh` can take seconds, so it lives on its own
/// thread and hands the collector a ready-made field map. Cached values are
/// sent before the first fetch, so PR badges and tickets appear immediately
/// (and survive being offline).
fn run_integrations(mut settings: Settings, cmds: Receiver<Cmd>, tx: Sender<IntegrationUpdate>) {
    let conn = match db::open() {
        Ok(c) => c,
        Err(e) => {
            warn!("integrations", "Cannot open database: {e}");
            return;
        }
    };
    let mut ints = Integrations::new();
    ints.hydrate(&conn);
    let (mut last_gh, mut last_linear): (Option<Instant>, Option<Instant>) = (None, None);
    let mut known_branches: Vec<(String, String)> = Vec::new();
    let mut first = true;

    loop {
        let mut force = false;
        match cmds.recv_timeout(Duration::from_secs(1)) {
            Ok(Cmd::Quit) | Err(RecvTimeoutError::Disconnected) => return,
            Ok(Cmd::Refresh { force: f }) => force = f,
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
                settings = *s;
                if enabled_now {
                    last_gh = None;
                    last_linear = None;
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

        let gh_due = settings.gh_pr_status
            && ((force && settings.gh_refresh_on_manual) || new_branches || last_gh.is_none_or(|t| t.elapsed() >= gh_every));
        let linear_due = linear_on
            && ((force && settings.linear_refresh_on_manual) || new_branches || last_linear.is_none_or(|t| t.elapsed() >= linear_every));

        if !(gh_due || linear_due || first || new_branches) {
            continue;
        }
        if first {
            // Publish what's cached before the (slow) first fetch.
            let _ = tx.send(update(&ints, &branches));
            first = false;
        }
        if gh_due {
            let started = Instant::now();
            let outcome = ints.refresh_github(&conn, &groups, force);
            info!("integrations", "github refresh: {outcome:?} in {:?}", started.elapsed());
            last_gh = Some(Instant::now());
        }
        if linear_due {
            let started = Instant::now();
            let outcome = ints.refresh_linear(&conn, &settings.linear_api_key, &groups, force);
            info!("integrations", "linear refresh: {outcome:?} in {:?}", started.elapsed());
            last_linear = Some(Instant::now());
        }
        known_branches = branches.clone();
        if settings.linear_enabled && settings.linear_auto_nickname {
            auto_nickname(&conn, &ints, &worktrees);
        }
        let _ = tx.send(update(&ints, &branches));
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

fn update(ints: &Integrations, branches: &[(String, String)]) -> IntegrationUpdate {
    let fields = branches.iter().map(|(r, b)| ((r.clone(), b.clone()), ints.fields_for(r, b))).collect();
    let h = ints.health();
    IntegrationUpdate { fields, health: Health { github_failing: h.github_failing, linear_failing: h.linear_failing } }
}
