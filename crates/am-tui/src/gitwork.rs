//! Git status for the collector, run on its own thread.
//!
//! A full sweep used to run `git status` + `git log` for every worktree at
//! once on the collector thread: ~8 s with 56 worktrees in a large repo,
//! during which hook updates waited. Now the collector hands [`Job`]s to a
//! `git` thread and keeps going; it runs them [`WORKERS`] at a time and sends
//! a [`Batch`] back per chunk. [`Tracker`] decides what is due: on a full
//! sweep only worktrees whose [`gitinfo::signature`] moved (all of them every
//! [`DEEP_EVERY`]th sweep, for edits that touch no git file), plus new rows
//! and rows with agent activity, as before.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use am_core::gitinfo::{self, Signature};
use am_core::model::{CommitInfo, GitStatus, Worktree};
use am_core::warn;

/// `git` processes at once: enough to overlap, few enough not to thrash the disk.
pub const WORKERS: usize = 6;
/// Every this many full sweeps, refresh every worktree whatever its signature.
pub const DEEP_EVERY: u64 = 4;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct GitData {
    pub status: Option<GitStatus>,
    pub commit: Option<CommitInfo>,
}

#[derive(Debug, Clone)]
pub struct Job {
    pub id: String,
    pub path: PathBuf,
    /// The last commit seen, reused when HEAD hasn't moved (no `git log`).
    pub prev: Option<CommitInfo>,
}

pub struct Batch {
    pub results: Vec<(String, GitData)>,
    pub elapsed: Duration,
}

/// What the collector knows about each worktree's git state.
#[derive(Default)]
pub struct Tracker {
    data: HashMap<String, GitData>,
    sigs: HashMap<String, Option<Signature>>,
    /// Sent to the git thread, no result yet.
    pending: HashSet<String>,
    sweeps: u64,
}

impl Tracker {
    pub fn get(&self, id: &str) -> Option<&GitData> {
        self.data.get(id)
    }

    /// Jobs for this cycle: rows never refreshed, rows `moved` (agent
    /// activity) and, when `full`, rows whose `sig` changed (every row on
    /// every [`DEEP_EVERY`]th full sweep). Rows already in flight are skipped.
    pub fn due(
        &mut self,
        worktrees: &[Worktree],
        full: bool,
        moved: impl Fn(&Worktree) -> bool,
        sig: impl Fn(&Path) -> Option<Signature>,
    ) -> Vec<Job> {
        let deep = full && self.sweeps.is_multiple_of(DEEP_EVERY);
        if full {
            self.sweeps += 1;
            let ids: HashSet<&str> = worktrees.iter().map(|w| w.id.as_str()).collect();
            self.data.retain(|id, _| ids.contains(id.as_str()));
            self.sigs.retain(|id, _| ids.contains(id.as_str()));
        }
        let mut jobs = Vec::new();
        for w in worktrees {
            if self.pending.contains(&w.id) {
                continue;
            }
            let known = self.data.contains_key(&w.id);
            let due = !known || moved(w) || deep || (full && {
                let now = sig(Path::new(&w.path));
                self.sigs.get(&w.id) != Some(&now)
            });
            if !due {
                continue;
            }
            // Taken before git runs, so a change made while it runs shows up next sweep.
            self.sigs.insert(w.id.clone(), sig(Path::new(&w.path)));
            self.pending.insert(w.id.clone());
            jobs.push(Job { id: w.id.clone(), path: PathBuf::from(&w.path), prev: self.data.get(&w.id).and_then(|g| g.commit.clone()) });
        }
        jobs
    }

    /// Rows sent to the git thread so far and still in flight (tests).
    #[cfg(test)]
    pub fn in_flight(&self) -> usize {
        self.pending.len()
    }

    /// Make the next full sweep refresh every row (`r`).
    pub fn deep_next(&mut self) {
        self.sweeps = 0;
    }

    /// Take a batch's results; `true` when any row's data changed.
    pub fn absorb(&mut self, results: Vec<(String, GitData)>) -> bool {
        let mut changed = false;
        for (id, g) in results {
            self.pending.remove(&id);
            if self.data.get(&id) != Some(&g) {
                changed = true;
                self.data.insert(id, g);
            }
        }
        changed
    }
}

/// Starts the `git` thread. It ends when the job sender is dropped.
pub fn spawn() -> (Sender<Vec<Job>>, Receiver<Batch>) {
    let (jobs_tx, jobs_rx) = mpsc::channel::<Vec<Job>>();
    let (done_tx, done_rx) = mpsc::channel();
    let spawned = std::thread::Builder::new().name("git".into()).spawn(move || {
        let mut queue = VecDeque::new();
        loop {
            if queue.is_empty() {
                match jobs_rx.recv() {
                    Ok(jobs) => enqueue(&mut queue, jobs),
                    Err(_) => return,
                }
            }
            for more in jobs_rx.try_iter() {
                enqueue(&mut queue, more);
            }
            let chunk: Vec<Job> = queue.drain(..WORKERS.min(queue.len())).collect();
            let started = Instant::now();
            let results = run(&chunk, |j| {
                let (status, commit) = gitinfo::refresh(&j.path, j.prev.as_ref());
                GitData { status, commit }
            });
            if done_tx.send(Batch { results, elapsed: started.elapsed() }).is_err() {
                return;
            }
        }
    });
    if let Err(e) = spawned {
        warn!("tui", "Cannot start the git thread: {e}");
    }
    (jobs_tx, done_rx)
}

/// The git thread works through its queue one chunk of [`WORKERS`] at a
/// time, so a small request (agent activity on a row, a new worktree) jumps
/// ahead of a sweep in progress instead of waiting for all of it.
fn enqueue(queue: &mut VecDeque<Job>, jobs: Vec<Job>) {
    if jobs.len() <= WORKERS {
        for j in jobs.into_iter().rev() {
            queue.push_front(j);
        }
    } else {
        queue.extend(jobs);
    }
}

/// `f` over `jobs` on up to [`WORKERS`] threads.
fn run(jobs: &[Job], f: impl Fn(&Job) -> GitData + Sync) -> Vec<(String, GitData)> {
    let next = AtomicUsize::new(0);
    std::thread::scope(|s| {
        let workers: Vec<_> = (0..WORKERS.min(jobs.len()))
            .map(|_| {
                s.spawn(|| {
                    let mut out = Vec::new();
                    while let Some(j) = jobs.get(next.fetch_add(1, Ordering::Relaxed)) {
                        out.push((j.id.clone(), f(j)));
                    }
                    out
                })
            })
            .collect();
        workers.into_iter().filter_map(|h| h.join().ok()).flatten().collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn wt(id: &str) -> Worktree {
        Worktree {
            id: id.into(),
            repo_id: "r".into(),
            path: format!("/r/{id}"),
            branch: id.into(),
            name: id.into(),
            custom_name: None,
            is_main: false,
            created_at: String::new(),
            tmp_space: false,
        }
    }

    fn ids(jobs: &[Job]) -> Vec<&str> {
        jobs.iter().map(|j| j.id.as_str()).collect()
    }

    fn done(t: &mut Tracker, jobs: &[Job]) {
        t.absorb(jobs.iter().map(|j| (j.id.clone(), GitData::default())).collect());
    }

    #[test]
    fn unchanged_worktrees_are_skipped_between_deep_sweeps() {
        let rows: Vec<Worktree> = (0..5).map(|i| wt(&format!("w{i}"))).collect();
        let mut t = Tracker::default();
        let spawned = Cell::new(0);
        let sig = |_: &Path| None;
        let sweep = |t: &mut Tracker| {
            let jobs = t.due(&rows, true, |_| false, sig);
            spawned.set(spawned.get() + jobs.len());
            done(t, &jobs);
        };
        sweep(&mut t); // first (deep) sweep: everything
        assert_eq!(spawned.get(), 5);
        for _ in 1..DEEP_EVERY {
            sweep(&mut t);
        }
        assert_eq!(spawned.get(), 5, "nothing changed, nothing re-run");
        sweep(&mut t); // the next deep sweep
        assert_eq!(spawned.get(), 10);
    }

    #[test]
    fn a_changed_signature_new_row_or_agent_activity_is_due() {
        let rows = vec![wt("a"), wt("b"), wt("c")];
        let mut t = Tracker::default();
        let jobs = t.due(&rows, true, |_| false, |_| None);
        done(&mut t, &jobs);

        // b's git files moved.
        let sig = |p: &Path| p.ends_with("b").then(|| gitinfo::signature(Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap())).flatten();
        let jobs = t.due(&rows, true, |_| false, sig);
        assert_eq!(ids(&jobs), ["b"]);
        done(&mut t, &jobs);

        // Between full sweeps only activity and new rows count.
        let mut more = rows.clone();
        more.push(wt("d"));
        let jobs = t.due(&more, false, |w| w.id == "a", |_| None);
        assert_eq!(ids(&jobs), ["a", "d"]);
    }

    #[test]
    fn rows_in_flight_are_not_sent_twice() {
        let rows = vec![wt("a")];
        let mut t = Tracker::default();
        assert_eq!(t.due(&rows, true, |_| true, |_| None).len(), 1);
        assert!(t.due(&rows, true, |_| true, |_| None).is_empty());
        assert!(t.absorb(vec![("a".into(), GitData { status: Some(GitStatus::default()), commit: None })]));
        assert_eq!(t.due(&rows, false, |_| true, |_| None).len(), 1);
    }

    #[test]
    fn removed_worktrees_are_forgotten_on_a_full_sweep() {
        let mut t = Tracker::default();
        let jobs = t.due(&[wt("a"), wt("b")], true, |_| false, |_| None);
        done(&mut t, &jobs);
        t.due(&[wt("a")], true, |_| false, |_| None);
        assert!(t.get("b").is_none());
        assert!(t.get("a").is_some());
    }

    #[test]
    fn small_requests_jump_ahead_of_a_sweep() {
        let job = |id: &str| Job { id: id.into(), path: PathBuf::new(), prev: None };
        let mut q = VecDeque::new();
        enqueue(&mut q, (0..20).map(|i| job(&format!("s{i}"))).collect());
        enqueue(&mut q, vec![job("a"), job("b")]);
        let order: Vec<&str> = q.iter().map(|j| j.id.as_str()).collect();
        assert_eq!(&order[..3], ["a", "b", "s0"]);
        assert_eq!(order.len(), 22);
    }

    #[test]
    fn the_pool_runs_every_job_once() {
        let jobs: Vec<Job> = (0..40).map(|i| Job { id: format!("w{i}"), path: PathBuf::new(), prev: None }).collect();
        let mut out = run(&jobs, |_| GitData::default());
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out.dedup_by(|a, b| a.0 == b.0);
        assert_eq!(out.len(), 40);
    }
}
