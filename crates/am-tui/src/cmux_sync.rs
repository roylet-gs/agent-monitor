//! am's worktree state as pills on cmux's sidebar (`cmuxSync`): each cmux
//! workspace whose terminal is in a tracked worktree shows that worktree's
//! PR (number, CI, review), Linear ticket and running agent shell commands,
//! with the agent's context use as the workspace's progress bar. cmux shows
//! the branch and agent state itself. am's own workspace gets a summary
//! instead: how many agents wait, work or are done, review requests to do,
//! and PRs with failing CI ([`own_pills`]).
//!
//! The collector computes what each worktree should show ([`desired`]) and
//! sends it on change; a `cmux-sync` thread maps workspaces to worktrees and
//! runs `cmux set-status`/`clear-status`/`set-progress` only for what differs
//! from what it last pushed. It re-lists workspaces every [`RELIST`], so a
//! workspace opened in a worktree picks its pills up without a change in am.
//! Only inside cmux (`am_core::cmux::inside`): elsewhere the socket refuses.

use std::collections::{BTreeMap, HashMap};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use am_core::cmux::{self, CmuxRunner, Pill, Workspace};
use am_core::model::{ChecksStatus, Repository, ReviewRequest, WorktreeView};
use am_core::review_groups::QueryHits;
use am_core::status::Status;
use am_core::settings::Settings;
use am_core::{debug, info, warn};

use crate::ui::{Review, review_state};

/// How often workspaces are listed again (and pills re-checked).
const RELIST: Duration = Duration::from_secs(10);

const GREEN: &str = "#34c759";
const RED: &str = "#ff3b30";
const YELLOW: &str = "#ffcc00";
const PURPLE: &str = "#af52de";
const GREY: &str = "#8e8e93";
const BLUE: &str = "#0a84ff";

/// What one worktree's workspace shows: pills by key, and the progress bar.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WorktreePills {
    pub pills: BTreeMap<&'static str, Pill>,
    pub progress: Option<(f64, String)>,
}

/// What every workspace should show.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Desired {
    /// Worktree path → what its workspace should show. Every worktree is
    /// listed, also those with nothing to show, so a workspace in a nested
    /// worktree is matched to that worktree and never to the repo around it.
    pub worktrees: BTreeMap<String, WorktreePills>,
    /// am's own workspace: the summary, whatever directory it is in.
    pub own: WorktreePills,
}

fn pill(value: String, color: Option<&'static str>, priority: i32) -> Pill {
    Pill { value, icon: None, color, priority }
}

/// `#42 ✓ approved`, coloured like the dashboard's PR cell.
fn pr_pill(pr: &am_core::model::PrInfo) -> Pill {
    if pr.state == "MERGED" {
        return pill(format!("#{} merged", pr.number), Some(PURPLE), 30);
    }
    let checks = match pr.checks_status {
        ChecksStatus::Passing => " ✓",
        ChecksStatus::Failing => " ✗",
        ChecksStatus::Pending => " ◌",
        ChecksStatus::None => "",
    };
    let review = match review_state(pr) {
        Some(Review::Ready | Review::Approved) => " approved",
        Some(Review::Conflicts) => " conflicts",
        Some(Review::ChangesRequested) => " changes",
        Some(Review::Pending) => " review",
        Some(Review::Draft) => " draft",
        Some(Review::Closed) => " closed",
        None => "",
    };
    let color = match (pr.state.as_str(), pr.checks_status) {
        ("CLOSED", _) => GREY,
        (_, ChecksStatus::Failing) => RED,
        _ if review_state(pr) == Some(Review::Conflicts) => RED,
        _ if pr.is_draft => GREY,
        (_, ChecksStatus::Pending) => YELLOW,
        _ => GREEN,
    };
    pill(format!("#{}{checks}{review}", pr.number), Some(color), 30)
}

/// What `v`'s workspace should show.
pub fn worktree_pills(v: &WorktreeView, settings: &Settings) -> WorktreePills {
    let mut out = WorktreePills::default();
    if let Some(pr) = &v.pr {
        out.pills.insert("am_pr", pr_pill(pr));
    }
    if let Some(t) = &v.linear {
        out.pills.insert("am_ticket", pill(t.identifier.clone(), None, 20));
    }
    if v.shell_commands > 0 {
        let n = v.shell_commands;
        out.pills.insert("am_cmds", pill(format!("$ {n} command{}", if n == 1 { "" } else { "s" }), None, 10));
    }
    if settings.show_context_window
        && let Some(c) = &v.context
    {
        let window = am_core::context::window_for(c.tokens, settings.context_window_tokens);
        let f = am_core::context::fraction(c.tokens, window);
        out.progress = Some((f, format!("ctx {}%", (f * 100.0).round() as u32)));
    }
    out
}

/// `2 waiting`, `3 working`, `1 done` (open sessions, by each worktree's
/// headline), `4 to review` and `1 CI failing`; counts of zero are left out.
pub fn own_pills(views: &[WorktreeView], to_review: Option<usize>) -> WorktreePills {
    let (mut waiting, mut working, mut done, mut failing) = (0, 0, 0, 0);
    for v in views {
        if am_core::grouping::is_open_now(v.agent.as_ref()) {
            match v.agent.as_ref().map(|a| a.status) {
                Some(Status::Waiting) => waiting += 1,
                Some(Status::Executing | Status::Planning | Status::Delegating) => working += 1,
                Some(Status::Done) => done += 1,
                _ => {}
            }
        }
        if v.pr.as_ref().is_some_and(|p| p.state == "OPEN" && p.checks_status == ChecksStatus::Failing) {
            failing += 1;
        }
    }
    let mut out = WorktreePills::default();
    let mut put = |key, n: usize, label: &str, color, priority| {
        if n > 0 {
            out.pills.insert(key, pill(format!("{n} {label}"), color, priority));
        }
    };
    put("am_waiting", waiting, "waiting", Some(YELLOW), 50);
    put("am_working", working, "working", Some(BLUE), 45);
    put("am_done", done, "done", Some(GREEN), 40);
    put("am_reviews", to_review.unwrap_or(0), "to review", Some(PURPLE), 35);
    put("am_ci", failing, "CI failing", Some(RED), 30);
    out
}

/// The review requests the dashboard's badge counts: those in groups with
/// `badge` on. `None` while review requests are off or not fetched yet.
pub fn to_review(list: Option<&[ReviewRequest]>, repos: &[Repository], settings: &Settings, hits: &QueryHits) -> Option<usize> {
    let list = list.filter(|_| settings.review_requests_on())?;
    let g = crate::reviews_ui::Grouping::new(settings, am_core::timefmt::now_unix(), hits);
    let (per, _) = crate::reviews_ui::counts(list, repos, g);
    let named = g.groups.iter().enumerate().filter(|(_, x)| x.badge && x.enabled).map(|(i, _)| per[i]);
    let default = (g.default.badge && g.default.enabled).then(|| per[g.groups.len()]);
    Some(named.chain(default).sum())
}

/// Every worktree (empty pills for one with nothing to show), plus am's own
/// workspace's summary.
pub fn desired(views: &[WorktreeView], to_review: Option<usize>, settings: &Settings) -> Desired {
    Desired {
        worktrees: views.iter().map(|v| (v.worktree.path.clone(), worktree_pills(v, settings))).collect(),
        own: own_pills(views, to_review),
    }
}

/// The pushing side: what each workspace was last given.
pub struct Sync<R: CmuxRunner> {
    runner: R,
    /// am's own workspace (`CMUX_WORKSPACE_ID`), which gets the summary.
    own: Option<String>,
    workspaces: Vec<Workspace>,
    listed: Option<Instant>,
    pushed: HashMap<String, WorktreePills>,
}

impl<R: CmuxRunner> Sync<R> {
    pub fn new(runner: R) -> Self {
        Sync { runner, own: cmux::own_workspace(), workspaces: Vec::new(), listed: None, pushed: HashMap::new() }
    }

    /// Bring every workspace in line with `desired`, listing workspaces again
    /// when the last listing is older than [`RELIST`].
    pub fn apply(&mut self, desired: &Desired, now: Instant) {
        if self.listed.is_none_or(|t| now.duration_since(t) >= RELIST) {
            match cmux::list_workspaces(&self.runner) {
                Ok(list) => {
                    self.workspaces = list;
                    self.listed = Some(now);
                }
                Err(e) => {
                    debug!("cmux", "Couldn't list workspaces: {e}");
                    return;
                }
            }
        }
        let roots: Vec<&str> = desired.worktrees.keys().map(String::as_str).collect();
        let none = WorktreePills::default();
        let mut pushed = HashMap::new();
        for w in &self.workspaces {
            let want = if self.own.as_deref() == Some(w.id.as_str()) {
                &desired.own
            } else {
                w.directory.as_deref().and_then(|d| cmux::owner_of(d, &roots)).and_then(|r| desired.worktrees.get(r)).unwrap_or(&none)
            };
            let had = self.pushed.get(&w.id).unwrap_or(&none);
            if want != had && self.push(&w.id, had, want).is_err() {
                // Unknown state: forget it so the next pass starts over.
                continue;
            }
            if *want != none {
                pushed.insert(w.id.clone(), want.clone());
            }
        }
        self.pushed = pushed;
    }

    fn push(&self, ws: &str, had: &WorktreePills, want: &WorktreePills) -> Result<(), String> {
        for (key, p) in &want.pills {
            if had.pills.get(key) != Some(p) {
                cmux::set_status(&self.runner, ws, key, p)?;
            }
        }
        for key in had.pills.keys().filter(|k| !want.pills.contains_key(*k)) {
            cmux::clear_status(&self.runner, ws, key)?;
        }
        if had.progress != want.progress {
            cmux::set_progress(&self.runner, ws, want.progress.as_ref().map(|(v, l)| (*v, l.as_str())))?;
        }
        debug!("cmux", "Synced pills for workspace {ws}: {:?}", want.pills.keys().collect::<Vec<_>>());
        Ok(())
    }

    /// Take every pill am put up down again (on exit or `cmuxSync` off).
    pub fn clear_all(&mut self) {
        let none = WorktreePills::default();
        for (ws, had) in std::mem::take(&mut self.pushed) {
            let _ = self.push(&ws, &had, &none);
        }
    }
}

/// The running `cmux-sync` thread.
pub struct Handle {
    tx: Sender<Desired>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Handle {
    /// Hand it the latest [`Desired`]; an empty one clears everything.
    pub fn send(&self, desired: Desired) {
        let _ = self.tx.send(desired);
    }

    /// Stop it and wait (at most `wait`) for it to take its pills down, so
    /// they don't outlive am when the process exits right after.
    pub fn stop(self, wait: Duration) {
        let Handle { tx, thread } = self;
        drop(tx);
        let Some(thread) = thread else { return };
        let deadline = Instant::now() + wait;
        while !thread.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        if thread.is_finished() {
            let _ = thread.join();
        } else {
            warn!("cmux", "Pills still being cleared after {wait:?}; leaving them");
        }
    }
}

/// Start the `cmux-sync` thread. It clears what it put up when stopped.
pub fn spawn() -> Handle {
    let (tx, rx) = std::sync::mpsc::channel::<Desired>();
    let thread = std::thread::Builder::new().name("cmux-sync".into()).spawn(move || run(rx)).ok();
    Handle { tx, thread }
}

/// Only one am-tui per data dir talks to cmux (pills and review
/// notifications): the one holding an advisory lock on
/// `<app dir>/cmux-sync.lock`. Otherwise two instances (`am cmux dock
/// --reviews`, a `ctrl-v` window, am in two terminals) would each post every
/// notification, and one quitting would clear pills the other thinks it set.
/// The lock goes with the process, so another instance takes over (retrying
/// every [`RELIST`]) once the owner quits.
pub mod role {
    use std::fs::File;
    use std::path::Path;
    use std::sync::Mutex;
    use std::time::Instant;

    use am_core::debug;

    struct Role {
        lock: Option<File>,
        tried: Option<Instant>,
        off: bool,
    }

    static ROLE: Mutex<Role> = Mutex::new(Role { lock: None, tried: None, off: false });

    /// This process never takes the role (a review window beside the
    /// dashboard).
    pub fn give_up() {
        if let Ok(mut r) = ROLE.lock() {
            r.off = true;
            r.lock = None;
        }
    }

    /// Whether this process is the one that talks to cmux, taking the role
    /// when it's free.
    pub fn owned() -> bool {
        let Ok(mut r) = ROLE.lock() else { return false };
        if r.off {
            return false;
        }
        if r.lock.is_some() {
            return true;
        }
        if r.tried.is_some_and(|t| t.elapsed() < super::RELIST) {
            return false;
        }
        r.tried = Some(Instant::now());
        r.lock = try_lock(&am_core::paths::app_dir().join("cmux-sync.lock"));
        r.lock.is_some()
    }

    /// The lock file, locked, or `None` when another holder has it.
    pub fn try_lock(path: &Path) -> Option<File> {
        let file = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(path).ok()?;
        match file.try_lock() {
            Ok(()) => {
                debug!("cmux", "Took the cmux role ({})", path.display());
                Some(file)
            }
            Err(_) => None,
        }
    }
}

fn run(rx: Receiver<Desired>) {
    info!("cmux", "Syncing worktree pills to cmux");
    let mut sync = Sync::new(cmux::SystemCmux);
    let mut desired = Desired::default();
    loop {
        match rx.recv_timeout(RELIST) {
            Ok(d) => {
                // Coalesce to the newest.
                desired = rx.try_iter().last().unwrap_or(d);
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                sync.clear_all();
                return;
            }
        }
        let started = Instant::now();
        sync.apply(&desired, started);
        if started.elapsed() > Duration::from_secs(2) {
            warn!("cmux", "Pill sync took {:?}", started.elapsed());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use am_core::cmux::CmuxOutput;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Fake {
        calls: Mutex<Vec<String>>,
        workspaces: String,
    }

    impl CmuxRunner for Fake {
        fn run(&self, args: &[String]) -> CmuxOutput {
            self.calls.lock().unwrap().push(args.join(" "));
            match args.get(1).map(String::as_str) {
                Some("window.list") => CmuxOutput::success(r#"{"windows":[{"id":"W"}]}"#),
                Some("workspace.list") => CmuxOutput::success(self.workspaces.clone()),
                _ => CmuxOutput::success("OK"),
            }
        }
    }

    impl Fake {
        fn take(&self) -> Vec<String> {
            std::mem::take(&mut *self.calls.lock().unwrap())
        }
    }

    fn wp(pr: &str, progress: Option<f64>) -> WorktreePills {
        let mut p = WorktreePills::default();
        p.pills.insert("am_pr", pill(pr.into(), Some(GREEN), 30));
        p.progress = progress.map(|v| (v, format!("ctx {}%", (v * 100.0) as u32)));
        p
    }

    fn fake() -> Fake {
        Fake {
            workspaces: r#"{"workspaces":[
                {"id":"A","current_directory":"/r/repo"},
                {"id":"B","current_directory":"/r/repo/.claude/worktrees/feat/src"},
                {"id":"C","current_directory":"/tmp"}
            ]}"#
            .into(),
            ..Default::default()
        }
    }

    #[test]
    fn pushes_only_what_changed() {
        let t0 = Instant::now();
        let mut sync = Sync::new(fake());
        let mut d = Desired::default();
        d.worktrees.insert("/r/repo".into(), wp("#1 ✓", None));
        d.worktrees.insert("/r/repo/.claude/worktrees/feat".into(), wp("#2 ◌", Some(0.5)));
        sync.apply(&d, t0);
        let calls = sync.runner.take();
        assert_eq!(&calls[..2], ["rpc window.list {}", r#"rpc workspace.list {"window_id":"W"}"#]);
        assert_eq!(
            &calls[2..],
            [
                "set-status am_pr #1 ✓ --workspace A --color #34c759 --priority 30",
                "set-status am_pr #2 ◌ --workspace B --color #34c759 --priority 30",
                "set-progress 0.50 --label ctx 50% --workspace B",
            ]
        );

        // Nothing changed and the listing is fresh: no cmux calls at all.
        sync.apply(&d, t0 + Duration::from_secs(1));
        assert!(sync.runner.take().is_empty());

        // The feature's PR goes away and its context bar with it; main is untouched.
        d.worktrees.insert("/r/repo/.claude/worktrees/feat".into(), {
            let mut p = WorktreePills::default();
            p.pills.insert("am_ticket", pill("ENG-1".into(), None, 20));
            p
        });
        sync.apply(&d, t0 + Duration::from_secs(2));
        assert_eq!(
            sync.runner.take(),
            [
                "set-status am_ticket ENG-1 --workspace B --priority 20",
                "clear-status am_pr --workspace B",
                "clear-progress --workspace B",
            ]
        );

        // Turned off: everything comes down.
        sync.clear_all();
        let mut cleared = sync.runner.take();
        cleared.sort();
        assert_eq!(cleared, ["clear-status am_pr --workspace A", "clear-status am_ticket --workspace B"]);
    }

    #[test]
    fn a_nested_worktree_with_nothing_to_show_never_gets_its_repos_pills() {
        let mut sync = Sync::new(fake());
        let mut d = Desired::default();
        d.worktrees.insert("/r/repo".into(), wp("#1 ✓", Some(0.4)));
        d.worktrees.insert("/r/repo/.claude/worktrees/feat".into(), WorktreePills::default());
        sync.apply(&d, Instant::now());
        let calls = sync.runner.take();
        assert_eq!(
            &calls[2..],
            ["set-status am_pr #1 ✓ --workspace A --color #34c759 --priority 30", "set-progress 0.40 --label ctx 40% --workspace A"]
        );
        assert!(!sync.pushed.contains_key("B"));
    }

    #[test]
    fn only_one_holder_takes_the_role() {
        let dir = std::env::temp_dir().join(format!("am-cmux-role-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cmux-sync.lock");
        let first = role::try_lock(&path).expect("free lock");
        assert!(role::try_lock(&path).is_none());
        drop(first);
        assert!(role::try_lock(&path).is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stop_waits_for_the_thread() {
        // Nothing sent, so the thread never runs cmux: it just sees the
        // sender go and returns.
        let h = spawn();
        let started = Instant::now();
        h.stop(Duration::from_secs(2));
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    fn view(id: &str, status: Option<Status>, ci: Option<ChecksStatus>) -> WorktreeView {
        use am_core::model::{AgentState, PrInfo, Worktree};
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
            agent: status.map(|status| AgentState {
                status,
                session_id: Some(id.into()),
                last_response: None,
                is_open: true,
                active_subagents: 0,
                updated_at: "2026-10-01 00:00:00".into(),
                agent: Default::default(),
            }),
            git: None,
            last_commit: None,
            pr: ci.map(|checks_status| PrInfo {
                number: 1,
                title: String::new(),
                url: String::new(),
                state: "OPEN".into(),
                is_draft: false,
                review_decision: String::new(),
                has_feedback: false,
                checks_status,
                active_check_url: None,
                active_check_name: None,
                checks_waiting: false,
                base_ref_name: None,
                mergeable: None,
            }),
            linear: None,
            has_terminal: false,
            open_ide: None,
            running_processes: Vec::new(),
            shell_commands: 0,
            context: None,
            sessions: Vec::new(),
        }
    }

    #[test]
    fn own_pills_sum_up_agents_reviews_and_failing_ci() {
        let views = [
            view("a", Some(Status::Waiting), Some(ChecksStatus::Failing)),
            view("b", Some(Status::Executing), Some(ChecksStatus::Passing)),
            view("c", Some(Status::Delegating), None),
            view("d", Some(Status::Idle), None),
            view("e", None, None),
        ];
        let p = own_pills(&views, Some(4));
        let got: Vec<(&str, &str)> = p.pills.iter().map(|(k, v)| (*k, v.value.as_str())).collect();
        assert_eq!(got, [("am_ci", "1 CI failing"), ("am_reviews", "4 to review"), ("am_waiting", "1 waiting"), ("am_working", "2 working")]);
        assert!(p.progress.is_none());
        // Nothing to say: no pills at all.
        assert_eq!(own_pills(&views[3..], Some(0)), WorktreePills::default());
    }

    #[test]
    fn ams_own_workspace_gets_the_summary_wherever_it_is() {
        let mut sync = Sync::new(fake());
        sync.own = Some("C".into());
        let mut d = Desired::default();
        d.own.pills.insert("am_waiting", pill("1 waiting".into(), Some(YELLOW), 50));
        sync.apply(&d, Instant::now());
        assert_eq!(&sync.runner.take()[2..], ["set-status am_waiting 1 waiting --workspace C --color #ffcc00 --priority 50"]);
        sync.clear_all();
        assert_eq!(sync.runner.take(), ["clear-status am_waiting --workspace C"]);
    }

    #[test]
    fn a_failed_listing_pushes_nothing() {
        struct Down;
        impl CmuxRunner for Down {
            fn run(&self, _: &[String]) -> CmuxOutput {
                CmuxOutput::failure("Error: Failed to write to socket (Broken pipe, errno 32)")
            }
        }
        let mut sync = Sync::new(Down);
        let mut d = Desired::default();
        d.worktrees.insert("/r/repo".into(), wp("#1", None));
        sync.apply(&d, Instant::now());
        assert!(sync.pushed.is_empty());
    }
}
