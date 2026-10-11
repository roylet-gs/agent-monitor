//! Background collector: builds `Snapshot`s off the UI thread.
//!
//! Change detection is `PRAGMA data_version` on a long-lived connection —
//! it bumps whenever any *other* connection (a hook process) commits, so the
//! dashboard reacts to agent events within ~200 ms without owning the pub/sub
//! socket (which the TS TUI may hold during the migration) and without
//! rescanning git. Git is refreshed for the worktrees whose agent just did
//! something, plus a full sweep every polling interval.

use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use am_core::model::{CommitInfo, GitStatus, Worktree, WorktreeView};
use am_core::settings::Settings;
use am_core::{db, debug, gitinfo, grouping, warn};
use rusqlite::Connection;

use crate::app::{Health, Snapshot};

const TICK: Duration = Duration::from_millis(200);
/// Recently finished subagents stay visible this long.
const RECENT_SUBAGENT_MINUTES: u32 = 30;

enum Cmd {
    Refresh { force: bool },
    Quit,
}

pub struct Collector {
    rx: Receiver<Snapshot>,
    cmd: Sender<Cmd>,
    handle: Option<JoinHandle<()>>,
}

impl Collector {
    pub fn spawn(settings: Settings) -> Collector {
        let (tx, rx) = mpsc::channel();
        let (cmd, cmd_rx) = mpsc::channel();
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
                Worker::new(settings, conn, tx).run(cmd_rx);
            })
            .expect("spawn collector");
        Collector { rx, cmd, handle: Some(handle) }
    }

    pub fn try_recv(&self) -> Option<Snapshot> {
        // Coalesce: only the newest snapshot matters.
        self.rx.try_iter().last()
    }

    pub fn refresh(&self, force: bool) {
        let _ = self.cmd.send(Cmd::Refresh { force });
    }

    pub fn shutdown(mut self) {
        let _ = self.cmd.send(Cmd::Quit);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
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
    git: HashMap<String, GitData>,
    agent_seen: HashMap<String, String>,
    last_full: Option<Instant>,
    data_version: i64,
    last_sent: Option<Snapshot>,
}

impl Worker {
    fn new(settings: Settings, conn: Connection, tx: Sender<Snapshot>) -> Worker {
        Worker {
            settings,
            conn,
            tx,
            git: HashMap::new(),
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
                Err(RecvTimeoutError::Timeout) => {}
            }
            if let Err(e) = self.cycle(force) {
                warn!("tui", "Refresh failed: {e}");
            }
        }
    }

    fn cycle(&mut self, force: bool) -> rusqlite::Result<()> {
        let full_due = force || self.last_full.is_none_or(|t| t.elapsed() >= self.poll_interval());
        let dv = db::data_version(&self.conn)?;
        if !full_due && dv == self.data_version {
            return Ok(());
        }
        self.data_version = dv;

        let worktrees = db::worktrees(&self.conn)?;
        let agents = db::agent_states(&self.conn)?;

        // Which worktrees need fresh git data this cycle?
        let stale: Vec<&Worktree> = if full_due {
            worktrees.iter().collect()
        } else {
            worktrees
                .iter()
                .filter(|w| {
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
                WorktreeView {
                    agent: agents.get(&w.id).cloned(),
                    git: g.status,
                    last_commit: g.commit,
                    pr: None,
                    linear: None,
                    has_terminal: false,
                    open_ide: None,
                    running_processes: Vec::new(),
                    worktree: w,
                }
            })
            .collect();

        let repos = db::repositories(&self.conn)?;
        let groups = grouping::build_groups(&repos, grouping::apply_filters(views, &self.settings), &self.settings);
        let subagents = db::subagents(&self.conn, None, RECENT_SUBAGENT_MINUTES)?;
        let snapshot = Snapshot { groups, subagents, health: Health::default() };
        if self.last_sent.as_ref() != Some(&snapshot) {
            let _ = self.tx.send(snapshot.clone());
            self.last_sent = Some(snapshot);
        }
        Ok(())
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
