//! am's worktree state as pills on cmux's sidebar (`cmuxSync`): each cmux
//! workspace whose terminal is in a tracked worktree shows that worktree's
//! PR (number, CI, review), Linear ticket and running agent shell commands,
//! with the agent's context use as the workspace's progress bar. cmux shows
//! the branch and agent state itself. am's own workspace gets a summary
//! instead: how many agents wait, work or are done, review requests to do
//! (and how many ask you by name), PRs with failing CI, and how many
//! worktrees have a live agent as its progress bar ([`own_pills`]). Every
//! pill carries an SF Symbol icon.
//!
//! The collector computes what each worktree should show ([`desired`]) and
//! sends it on change; a `cmux-sync` thread maps workspaces to worktrees and
//! runs `cmux set-status`/`clear-status`/`set-progress` only for what differs
//! from what it last pushed. A workspace an agent's hooks reported running in
//! (`CMUX_WORKSPACE_ID`, `Desired::pinned`) belongs to that agent's worktree
//! whatever directory its focused pane is in; any other goes by directory.
//! Workspaces are listed again when cmux's event stream says one opened,
//! closed or moved (`cmux events`, [`cmux::WORKSPACE_EVENTS`]), and every
//! [`RELIST`] in case the stream isn't there.
//!
//! Changes worth a look ([`alerts`], per `cmuxNotify`: an agent waiting or
//! done, CI starting to fail, a PR approved or given changes, conflicts) go
//! to the worktree's own workspace as a notification (on the agent's pane
//! when known) plus a sidebar log line.
//! Only inside cmux (`am_core::cmux::inside`): elsewhere the socket refuses.

use std::collections::{BTreeMap, HashMap};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use am_core::cmux::{self, CmuxRunner, LogLevel, Pill, Workspace};
use am_core::db::CmuxPane;
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
    /// Workspace id → the worktree path an agent in it reported from.
    pub pinned: BTreeMap<String, String>,
}

fn pill(value: String, icon: &'static str, color: Option<&'static str>, priority: i32) -> Pill {
    Pill { value, icon: Some(icon), color, priority }
}

/// `#42 ✓ approved`, coloured like the dashboard's PR cell.
fn pr_pill(pr: &am_core::model::PrInfo) -> Pill {
    if pr.state == "MERGED" {
        return pill(format!("#{} merged", pr.number), "arrow.triangle.merge", Some(PURPLE), 30);
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
    let icon = if pr.state == "CLOSED" { "xmark.circle" } else { "arrow.triangle.pull" };
    pill(format!("#{}{checks}{review}", pr.number), icon, Some(color), 30)
}

/// What `v`'s workspace should show.
pub fn worktree_pills(v: &WorktreeView, settings: &Settings) -> WorktreePills {
    let mut out = WorktreePills::default();
    if let Some(pr) = &v.pr {
        out.pills.insert("am_pr", pr_pill(pr));
    }
    if let Some(t) = &v.linear {
        out.pills.insert("am_ticket", pill(t.identifier.clone(), "tag.fill", None, 20));
    }
    if v.shell_commands > 0 {
        let n = v.shell_commands;
        out.pills.insert("am_cmds", pill(format!("{n} command{}", if n == 1 { "" } else { "s" }), "terminal.fill", None, 10));
    }
    if settings.show_context_window
        && let Some(c) = &v.context
    {
        let window = c.window(settings.context_window_tokens);
        let f = am_core::context::fraction(c.tokens, window);
        out.progress = Some((f, format!("ctx {}%", (f * 100.0).round() as u32)));
    }
    out
}

/// Review requests the dashboard's badge counts, and how many of them ask
/// you by name.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReviewCount {
    pub total: usize,
    pub you: usize,
}

/// `2 waiting`, `3 working`, `1 done` (open sessions, by each worktree's
/// headline), `4 to review · 2 you` and `1 CI failing`, counts of zero left
/// out; the progress bar says how many worktrees have a live agent.
pub fn own_pills(views: &[WorktreeView], to_review: Option<ReviewCount>) -> WorktreePills {
    let (mut waiting, mut working, mut done, mut failing, mut active) = (0, 0, 0, 0, 0);
    for v in views {
        if am_core::grouping::is_open_now(v.agent.as_ref()) {
            active += 1;
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
    let mut put = |key, n: usize, label: String, icon, color, priority| {
        if n > 0 {
            out.pills.insert(key, pill(format!("{n} {label}"), icon, color, priority));
        }
    };
    let reviews = to_review.unwrap_or_default();
    let you = if reviews.you > 0 { format!(" · {} you", reviews.you) } else { String::new() };
    put("am_waiting", waiting, "waiting".into(), "hand.raised.fill", Some(YELLOW), 50);
    put("am_reviews", reviews.total, format!("to review{you}"), "eye.fill", Some(PURPLE), 48);
    put("am_working", working, "working".into(), "hammer.fill", Some(BLUE), 45);
    put("am_done", done, "done".into(), "checkmark.circle.fill", Some(GREEN), 40);
    put("am_ci", failing, "CI failing".into(), "xmark.octagon.fill", Some(RED), 30);
    if !views.is_empty() {
        let total = views.len();
        out.progress = Some((active as f64 / total as f64, format!("{active} of {total} worktrees active")));
    }
    out
}

/// The review requests the dashboard's badge counts: those in groups with
/// `badge` on, and of those the ones asking you by name. `None` while review
/// requests are off or not fetched yet.
pub fn to_review(list: Option<&[ReviewRequest]>, repos: &[Repository], settings: &Settings, hits: &QueryHits) -> Option<ReviewCount> {
    let list = list.filter(|_| settings.review_requests_on())?;
    let g = crate::reviews_ui::Grouping::new(settings, am_core::timefmt::now_unix(), hits);
    let badged = |i: usize| match g.groups.get(i) {
        Some(x) => x.badge && x.enabled,
        None => i == g.groups.len() && g.default.badge && g.default.enabled,
    };
    let mut n = ReviewCount::default();
    for (r, grp) in list.iter().zip(crate::reviews_ui::groups_of(list, repos, g)) {
        if grp.is_some_and(badged) {
            n.total += 1;
            n.you += usize::from(crate::reviews_ui::asks_you(r));
        }
    }
    Some(n)
}

/// Every worktree (empty pills for one with nothing to show), plus am's own
/// workspace's summary.
pub fn desired(views: &[WorktreeView], to_review: Option<ReviewCount>, settings: &Settings) -> Desired {
    let mut pinned = BTreeMap::new();
    // Oldest activity first, so the newest session's worktree wins a workspace.
    let mut panes: Vec<(&str, &str, &str)> = views
        .iter()
        .flat_map(|v| v.sessions.iter().filter_map(move |s| Some((s.updated_at.as_str(), s.cmux.as_ref()?.workspace.as_str(), v.worktree.path.as_str()))))
        .collect();
    panes.sort();
    for (_, ws, path) in panes {
        pinned.insert(ws.to_owned(), path.to_owned());
    }
    Desired {
        worktrees: views.iter().map(|v| (v.worktree.path.clone(), worktree_pills(v, settings))).collect(),
        own: own_pills(views, to_review),
        pinned,
    }
}

/// A change worth a notification on a worktree's workspace.
#[derive(Debug, Clone, PartialEq)]
pub struct Alert {
    /// The worktree's path, to find its workspace by directory.
    pub path: String,
    /// The pane its agent runs in, when the hooks reported one.
    pub pane: Option<CmuxPane>,
    pub title: String,
    pub body: String,
    pub level: LogLevel,
}

/// What [`alerts`] remembers of a worktree between snapshots.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Seen {
    status: Option<Status>,
    /// PR number, CI failing, review verdict (`APPROVED`/`CHANGES_REQUESTED`), conflicts.
    pr: Option<(u64, bool, Option<String>, bool)>,
}

fn seen(v: &WorktreeView) -> Seen {
    Seen {
        status: v.status(),
        pr: v.pr.as_ref().filter(|p| p.state == "OPEN").map(|p| {
            let verdict = matches!(p.review_decision.as_str(), "APPROVED" | "CHANGES_REQUESTED").then(|| p.review_decision.clone());
            (p.number, p.checks_status == ChecksStatus::Failing, verdict, p.mergeable.as_deref() == Some("CONFLICTING"))
        }),
    }
}

/// The alerts `views` raise against what was seen before (`prev`, updated in
/// place), per `cmuxNotify`. Nothing on the first call (`prev` empty): that
/// only records the starting point. A PR only alerts while it stays the same
/// PR, so a fetch filling it in for the first time says nothing.
pub fn alerts(prev: &mut Option<HashMap<String, Seen>>, views: &[WorktreeView], settings: &Settings) -> Vec<Alert> {
    let now: HashMap<String, Seen> = views.iter().map(|v| (v.worktree.id.clone(), seen(v))).collect();
    let Some(before) = prev.replace(now) else { return Vec::new() };
    let mut out = Vec::new();
    for v in views {
        let (Some(was), Some(is)) = (before.get(&v.worktree.id), prev.as_ref().and_then(|p| p.get(&v.worktree.id))) else { continue };
        let name = v.worktree.display_name();
        let mut alert = |title: String, body: String, level| {
            out.push(Alert { path: v.worktree.path.clone(), pane: v.cmux_pane().cloned(), title, body, level });
        };
        let said = || {
            let s = v.agent.as_ref().and_then(|a| a.last_response.as_deref()).unwrap_or("").trim();
            am_core::status::truncate(s.lines().next().unwrap_or(""), 160)
        };
        if was.status != is.status {
            match is.status {
                Some(Status::Waiting) if settings.cmux_notifies("waiting") => {
                    let body = said();
                    alert(format!("{name} needs you"), if body.is_empty() { "Waiting for input".into() } else { body }, LogLevel::Warning);
                }
                Some(Status::Done)
                    if settings.cmux_notifies("done")
                        && matches!(was.status, Some(Status::Executing | Status::Planning | Status::Delegating)) =>
                {
                    alert(format!("{name} finished"), said(), LogLevel::Success);
                }
                _ => {}
            }
        }
        let (Some((n, was_failing, was_verdict, was_conflicts)), Some((m, failing, verdict, conflicts))) = (&was.pr, &is.pr) else { continue };
        if n != m {
            continue;
        }
        let title = v.pr.as_ref().map(|p| p.title.clone()).unwrap_or_default();
        if *failing && !was_failing && settings.cmux_notifies("ci") {
            let check = v.pr.as_ref().and_then(|p| p.active_check_name.clone());
            let body = match check {
                Some(c) => format!("{c} — {title}"),
                None => title.clone(),
            };
            alert(format!("CI failing on #{m} ({name})"), body, LogLevel::Error);
        }
        if verdict != was_verdict && settings.cmux_notifies("review") {
            match verdict.as_deref() {
                Some("APPROVED") => alert(format!("#{m} approved ({name})"), title.clone(), LogLevel::Success),
                Some("CHANGES_REQUESTED") => alert(format!("Changes requested on #{m} ({name})"), title.clone(), LogLevel::Warning),
                _ => {}
            }
        }
        if *conflicts && !was_conflicts && settings.cmux_notifies("conflicts") {
            alert(format!("#{m} has merge conflicts ({name})"), title.clone(), LogLevel::Warning);
        }
    }
    out
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
            } else if let Some(path) = desired.pinned.get(&w.id) {
                desired.worktrees.get(path).unwrap_or(&none)
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

    /// List workspaces again on the next [`apply`](Sync::apply); after a
    /// sidebar reset, push every pill again too.
    pub fn on_event(&mut self, ev: &cmux::Event) {
        debug!("cmux", "Event {} (workspace {:?}): listing workspaces again", ev.name, ev.workspace);
        self.listed = None;
        if ev.name == "sidebar.reset" {
            match &ev.workspace {
                Some(ws) => {
                    self.pushed.remove(ws);
                }
                None => self.pushed.clear(),
            }
        }
    }

    /// Post `alert` on its worktree's workspace (the agent's pane's, else
    /// the one whose directory is in the worktree), else on am's own: a
    /// notification plus a line in that workspace's sidebar log.
    pub fn deliver(&mut self, alert: &Alert, desired: &Desired, now: Instant) {
        if self.listed.is_none_or(|t| now.duration_since(t) >= RELIST)
            && let Ok(list) = cmux::list_workspaces(&self.runner)
        {
            self.workspaces = list;
            self.listed = Some(now);
        }
        let listed = |id: &str| self.workspaces.iter().any(|w| w.id == id);
        let roots: Vec<&str> = desired.worktrees.keys().map(String::as_str).collect();
        let (ws, surface) = match alert.pane.as_ref().filter(|p| listed(&p.workspace)) {
            Some(p) => (Some(p.workspace.clone()), p.surface.clone()),
            None => {
                let by_dir = self.workspaces.iter().find(|w| {
                    self.own.as_deref() != Some(w.id.as_str())
                        && w.directory.as_deref().and_then(|d| cmux::owner_of(d, &roots)) == Some(alert.path.as_str())
                });
                (by_dir.map(|w| w.id.clone()), None)
            }
        };
        let _ = cmux::notify(&self.runner, &alert.title, &alert.body, ws.as_deref(), surface.as_deref());
        if let Some(ws) = ws.as_deref().or(self.own.as_deref()) {
            let line = if alert.body.is_empty() { alert.title.clone() } else { format!("{}: {}", alert.title, alert.body) };
            if let Err(e) = cmux::log(&self.runner, alert.level, &line, ws) {
                debug!("cmux", "Couldn't log to workspace {ws}: {e}");
            }
        }
    }

    /// Take every pill am put up down again (on exit or `cmuxSync` off).
    pub fn clear_all(&mut self) {
        let none = WorktreePills::default();
        for (ws, had) in std::mem::take(&mut self.pushed) {
            let _ = self.push(&ws, &had, &none);
        }
    }
}

/// What the `cmux-sync` thread is told.
enum Msg {
    Desired(Desired),
    Alerts(Vec<Alert>),
    Event(cmux::Event),
    /// The collector is done (the events follower holds a sender too, so
    /// the channel never closes on its own).
    Quit,
}

/// The running `cmux-sync` thread.
pub struct Handle {
    tx: Sender<Msg>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Handle {
    /// Hand it the latest [`Desired`]; an empty one clears everything.
    pub fn send(&self, desired: Desired) {
        let _ = self.tx.send(Msg::Desired(desired));
    }

    /// Post these on their workspaces.
    pub fn alert(&self, alerts: Vec<Alert>) {
        if !alerts.is_empty() {
            let _ = self.tx.send(Msg::Alerts(alerts));
        }
    }

    /// Stop it and wait (at most `wait`) for it to take its pills down, so
    /// they don't outlive am when the process exits right after.
    pub fn stop(mut self, wait: Duration) {
        let _ = self.tx.send(Msg::Quit);
        let Some(thread) = self.thread.take() else { return };
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

impl Drop for Handle {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Quit);
    }
}

/// Start the `cmux-sync` thread. It clears what it put up when stopped.
pub fn spawn() -> Handle {
    let (tx, rx) = std::sync::mpsc::channel::<Msg>();
    let events = tx.clone();
    let thread = std::thread::Builder::new().name("cmux-sync".into()).spawn(move || run(rx, events)).ok();
    Handle { tx, thread }
}

/// The `cmux events` child [`follow_events`] reads, so [`run`] can stop it.
type Follower = std::sync::Arc<std::sync::Mutex<Option<std::process::Child>>>;

/// Follow `cmux events` for [`cmux::WORKSPACE_EVENTS`] on a `cmux-events`
/// thread, handing each to the sync thread until that is gone (it kills the
/// child on the way out). A stream that ends or can't start is tried again
/// after [`RELIST`]; the periodic listing covers the gap.
fn follow_events(tx: Sender<Msg>, child: Follower) {
    use std::io::BufRead;
    loop {
        match cmux::spawn_events(cmux::WORKSPACE_EVENTS) {
            Ok(mut c) => {
                debug!("cmux", "Following cmux events");
                let out = c.stdout.take();
                match child.lock() {
                    Ok(mut slot) => *slot = Some(c),
                    Err(_) => return,
                }
                for line in out.into_iter().flat_map(|o| std::io::BufReader::new(o).lines()) {
                    let Ok(line) = line else { break };
                    if let Some(ev) = cmux::parse_event(&line)
                        && tx.send(Msg::Event(ev)).is_err()
                    {
                        return;
                    }
                }
                if let Ok(mut slot) = child.lock()
                    && let Some(mut c) = slot.take()
                {
                    let _ = c.kill();
                    let _ = c.wait();
                }
                debug!("cmux", "cmux events stream ended");
            }
            Err(e) => debug!("cmux", "Couldn't follow cmux events: {e}"),
        }
        std::thread::sleep(RELIST);
        // The sync thread is gone: stop retrying.
        if tx.send(Msg::Alerts(Vec::new())).is_err() {
            return;
        }
    }
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

fn run(rx: Receiver<Msg>, events: Sender<Msg>) {
    info!("cmux", "Syncing worktree pills to cmux");
    let child: Follower = Default::default();
    // Outside cmux the stream can't connect (tests never spawn the real CLI).
    if cmux::inside() {
        let child = child.clone();
        let _ = std::thread::Builder::new().name("cmux-events".into()).spawn(move || follow_events(events, child));
    } else {
        drop(events);
    }
    let mut sync = Sync::new(cmux::SystemCmux);
    let mut desired = Desired::default();
    loop {
        let mut alerts = Vec::new();
        let mut quit = false;
        let mut take = |m: Msg, sync: &mut Sync<cmux::SystemCmux>| match m {
            Msg::Desired(d) => desired = d,
            Msg::Alerts(a) => alerts.extend(a),
            Msg::Event(e) => sync.on_event(&e),
            Msg::Quit => quit = true,
        };
        match rx.recv_timeout(RELIST) {
            Ok(m) => {
                take(m, &mut sync);
                // Coalesce to the newest.
                for m in rx.try_iter() {
                    take(m, &mut sync);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        if quit {
            break;
        }
        let started = Instant::now();
        sync.apply(&desired, started);
        for a in &alerts {
            sync.deliver(a, &desired, started);
        }
        if started.elapsed() > Duration::from_secs(2) {
            warn!("cmux", "Pill sync took {:?}", started.elapsed());
        }
    }
    sync.clear_all();
    if let Ok(mut slot) = child.lock()
        && let Some(mut c) = slot.take()
    {
        let _ = c.kill();
        let _ = c.wait();
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
        p.pills.insert("am_pr", pill(pr.into(), "arrow.triangle.pull", Some(GREEN), 30));
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
                "set-status am_pr #1 ✓ --workspace A --icon arrow.triangle.pull --color #34c759 --priority 30",
                "set-status am_pr #2 ◌ --workspace B --icon arrow.triangle.pull --color #34c759 --priority 30",
                "set-progress 0.50 --label ctx 50% --workspace B",
            ]
        );

        // Nothing changed and the listing is fresh: no cmux calls at all.
        sync.apply(&d, t0 + Duration::from_secs(1));
        assert!(sync.runner.take().is_empty());

        // The feature's PR goes away and its context bar with it; main is untouched.
        d.worktrees.insert("/r/repo/.claude/worktrees/feat".into(), {
            let mut p = WorktreePills::default();
            p.pills.insert("am_ticket", pill("ENG-1".into(), "tag.fill", None, 20));
            p
        });
        sync.apply(&d, t0 + Duration::from_secs(2));
        assert_eq!(
            sync.runner.take(),
            [
                "set-status am_ticket ENG-1 --workspace B --icon tag.fill --priority 20",
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
            ["set-status am_pr #1 ✓ --workspace A --icon arrow.triangle.pull --color #34c759 --priority 30", "set-progress 0.40 --label ctx 40% --workspace A"]
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
        let p = own_pills(&views, Some(ReviewCount { total: 4, you: 1 }));
        let got: Vec<(&str, &str)> = p.pills.iter().map(|(k, v)| (*k, v.value.as_str())).collect();
        assert_eq!(
            got,
            [("am_ci", "1 CI failing"), ("am_reviews", "4 to review · 1 you"), ("am_waiting", "1 waiting"), ("am_working", "2 working")]
        );
        assert!(p.pills.values().all(|p| p.icon.is_some()));
        // a, b and c have a live agent; d went idle long ago, e has none.
        assert_eq!(p.progress, Some((0.6, "3 of 5 worktrees active".into())));
        // No one asks you by name: the count alone.
        let p = own_pills(&views, Some(ReviewCount { total: 2, you: 0 }));
        assert_eq!(p.pills["am_reviews"].value, "2 to review");
        // Nothing to say: no pills, only the bar.
        let quiet = own_pills(&views[4..], Some(ReviewCount::default()));
        assert!(quiet.pills.is_empty());
        assert_eq!(quiet.progress, Some((0.0, "0 of 1 worktrees active".into())));
        assert_eq!(own_pills(&[], None), WorktreePills::default());
    }

    #[test]
    fn to_review_counts_the_badge_and_who_is_asked() {
        use crate::reviews_ui::tests::{review, review_asking};
        let settings = Settings { gh_pr_status: true, review_requests_enabled: true, ..Settings::default() };
        let list = [review("a", 1, "2026-10-01"), review_asking("a", 2, "2026-10-02", "@platform"), review("a", 3, "2026-10-03")];
        let hits = QueryHits::default();
        assert_eq!(to_review(Some(&list), &[], &settings, &hits), Some(ReviewCount { total: 3, you: 2 }));
        assert_eq!(to_review(None, &[], &settings, &hits), None);
        let off = Settings { review_requests_enabled: false, ..settings };
        assert_eq!(to_review(Some(&list), &[], &off, &hits), None);
    }

    #[test]
    fn ams_own_workspace_gets_the_summary_wherever_it_is() {
        let mut sync = Sync::new(fake());
        sync.own = Some("C".into());
        let mut d = Desired::default();
        d.own.pills.insert("am_waiting", pill("1 waiting".into(), "hand.raised.fill", Some(YELLOW), 50));
        sync.apply(&d, Instant::now());
        assert_eq!(&sync.runner.take()[2..], ["set-status am_waiting 1 waiting --workspace C --icon hand.raised.fill --color #ffcc00 --priority 50"]);
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

    fn in_pane(mut v: WorktreeView, ws: &str) -> WorktreeView {
        v.sessions = vec![am_core::model::SessionView {
            agent: Default::default(),
            session_id: v.worktree.id.clone(),
            status: v.status().unwrap_or(Status::Idle),
            last_response: None,
            started_at: "2026-10-01 00:00:00".into(),
            updated_at: "2026-10-01 00:00:00".into(),
            context: None,
            cmux: Some(CmuxPane { workspace: ws.into(), surface: Some(format!("{ws}-s")) }),
            waiting_on: None,
        }];
        v
    }

    #[test]
    fn a_workspace_an_agent_reported_from_shows_that_worktree() {
        // A's focused pane is at the repo root, but feat's agent runs there.
        let mut sync = Sync::new(fake());
        let mut d = desired(&[in_pane(view("feat", Some(Status::Executing), None), "A")], None, &Settings::default());
        assert_eq!(d.pinned.get("A").map(String::as_str), Some("/r/feat"));
        d.worktrees.insert("/r/repo".into(), wp("#1 ✓", None));
        d.worktrees.insert("/r/feat".into(), wp("#2 ◌", None));
        sync.apply(&d, Instant::now());
        let calls = sync.runner.take();
        assert!(calls.iter().any(|c| c.starts_with("set-status am_pr #2 ◌ --workspace A")), "{calls:?}");
        assert!(!calls.iter().any(|c| c.contains("#1 ✓ --workspace A")), "{calls:?}");
    }

    #[test]
    fn workspace_events_relist_and_a_sidebar_reset_repushes() {
        let t0 = Instant::now();
        let mut sync = Sync::new(fake());
        let mut d = Desired::default();
        d.worktrees.insert("/r/repo".into(), wp("#1 ✓", None));
        sync.apply(&d, t0);
        sync.runner.take();
        // A workspace opened: listed again at once, not after RELIST.
        sync.on_event(&cmux::Event { name: "workspace.created".into(), workspace: Some("D".into()) });
        sync.apply(&d, t0 + Duration::from_secs(1));
        assert_eq!(sync.runner.take().len(), 2, "relisted, nothing to push");
        // The sidebar was reset: A's pills are gone, so they go up again.
        sync.on_event(&cmux::Event { name: "sidebar.reset".into(), workspace: Some("A".into()) });
        sync.apply(&d, t0 + Duration::from_secs(2));
        assert!(sync.runner.take().iter().any(|c| c.starts_with("set-status am_pr #1 ✓ --workspace A")));
    }

    #[test]
    fn alerts_fire_on_transitions_per_cmux_notify() {
        let settings = Settings { cmux_notify: "waiting,done,ci,review,conflicts".into(), ..Settings::default() };
        let mut seen = None;
        let base = [view("a", Some(Status::Executing), Some(ChecksStatus::Pending)), view("b", Some(Status::Executing), None)];
        assert!(alerts(&mut seen, &base, &settings).is_empty(), "the first snapshot only sets the baseline");
        assert!(alerts(&mut seen, &base, &settings).is_empty());

        let mut a = in_pane(view("a", Some(Status::Done), Some(ChecksStatus::Failing)), "A");
        a.agent.as_mut().unwrap().last_response = Some("All tests pass.\nMore".into());
        let pr = a.pr.as_mut().unwrap();
        pr.title = "Add sync".into();
        pr.active_check_name = Some("test (ubuntu)".into());
        pr.review_decision = "CHANGES_REQUESTED".into();
        pr.mergeable = Some("CONFLICTING".into());
        let b = view("b", Some(Status::Waiting), None);
        let got = alerts(&mut seen, &[a.clone(), b.clone()], &settings);
        let titles: Vec<&str> = got.iter().map(|x| x.title.as_str()).collect();
        assert_eq!(titles, ["a finished", "CI failing on #1 (a)", "Changes requested on #1 (a)", "#1 has merge conflicts (a)", "b needs you"]);
        assert_eq!(got[0].body, "All tests pass.");
        assert_eq!(got[1].body, "test (ubuntu) — Add sync");
        assert_eq!(got[0].pane.as_ref().map(|p| p.workspace.as_str()), Some("A"));
        assert_eq!(got[4].body, "Waiting for input");
        assert!(alerts(&mut seen, &[a.clone(), b.clone()], &settings).is_empty(), "nothing new");

        // A different PR on the branch, or one fetched for the first time, says nothing.
        let mut other = a.clone();
        other.pr.as_mut().unwrap().number = 2;
        other.pr.as_mut().unwrap().checks_status = ChecksStatus::Pending;
        alerts(&mut seen, &[other.clone(), b.clone()], &settings);
        other.pr.as_mut().unwrap().number = 3;
        other.pr.as_mut().unwrap().checks_status = ChecksStatus::Failing;
        assert!(alerts(&mut seen, &[other, b.clone()], &settings).is_empty());

        // The defaults leave the agent ones out.
        let mut seen = None;
        alerts(&mut seen, &base, &Settings::default());
        let got = alerts(&mut seen, &[a, b], &Settings::default());
        assert_eq!(got.len(), 3, "{got:?}");
    }

    #[test]
    fn alerts_land_on_the_agents_pane_else_the_worktrees_workspace_else_ams() {
        let mut sync = Sync::new(fake());
        sync.own = Some("C".into());
        let mut d = Desired::default();
        d.worktrees.insert("/r/repo".into(), WorktreePills::default());
        d.worktrees.insert("/r/repo/.claude/worktrees/feat".into(), WorktreePills::default());
        let alert = |path: &str, pane: Option<&str>| Alert {
            path: path.into(),
            pane: pane.map(|w| CmuxPane { workspace: w.into(), surface: Some("S".into()) }),
            title: "CI failing on #2".into(),
            body: "test".into(),
            level: LogLevel::Error,
        };
        let now = Instant::now();
        sync.deliver(&alert("/r/repo/.claude/worktrees/feat", Some("A")), &d, now);
        sync.deliver(&alert("/r/repo/.claude/worktrees/feat", None), &d, now);
        sync.deliver(&alert("/r/nowhere", Some("gone")), &d, now);
        let calls: Vec<String> = sync.runner.take().into_iter().filter(|c| !c.starts_with("rpc")).collect();
        assert_eq!(
            calls,
            [
                "notify --title CI failing on #2 --body test --workspace A --surface S",
                "log --level error --source am --workspace A CI failing on #2: test",
                "notify --title CI failing on #2 --body test --workspace B",
                "log --level error --source am --workspace B CI failing on #2: test",
                "notify --title CI failing on #2 --body test",
                "log --level error --source am --workspace C CI failing on #2: test",
            ]
        );
    }
}
