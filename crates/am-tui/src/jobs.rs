//! Background jobs (worktree create / delete) shown in the right-hand jobs
//! pane. Pure state + key handling; the work itself runs in ops.rs and
//! reports back through `launcher::Msg`.
//!
//! The pane is visible while a job runs, waits for an answer (branch exists,
//! run the startup script?) or failed and hasn't been dismissed; a finished
//! job lingers for [`DONE_LINGER`] so its ✓ can be seen.

use std::time::{Duration, Instant};

use am_core::model::Repository;
use am_core::worktrees::{CreateMode, CreateRequest};

use crate::app::Key;

/// How long a successful job stays in the pane.
pub const DONE_LINGER: Duration = Duration::from_secs(3);
/// Longer for one with output (script output, delete warnings) to read.
pub const DONE_LINGER_LOG: Duration = Duration::from_secs(15);
/// Script output kept per job.
const LOG_CAP: usize = 500;

#[derive(Debug, Clone, PartialEq)]
pub enum JobKind {
    /// `worktree_id` is set once the worktree exists.
    Create { repo: Repository, req: CreateRequest, worktree_id: Option<String> },
    Delete { worktree_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobState {
    Running,
    Waiting,
    Done,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub text: String,
    pub done: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Question {
    BranchExists { local: bool, remote: bool },
    RunScript { repo_id: String, worktree_path: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Job {
    pub id: u64,
    pub kind: JobKind,
    pub label: String,
    pub steps: Vec<Step>,
    pub log: Vec<String>,
    pub question: Option<Question>,
    pub state: JobState,
    pub finished_at: Option<Instant>,
}

impl Job {
    pub fn current_step(&self) -> Option<&str> {
        self.steps.last().filter(|s| !s.done).map(|s| s.text.as_str())
    }

    /// A create that hasn't failed: the list shows a placeholder row for it
    /// until the snapshot has the worktree (`worktree_id`, once known).
    pub fn pending_create(&self) -> Option<(&Repository, &CreateRequest, Option<&str>)> {
        match &self.kind {
            JobKind::Create { repo, req, worktree_id } if !matches!(self.state, JobState::Failed(_)) => {
                Some((repo, req, worktree_id.as_deref()))
            }
            _ => None,
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum JobEffect {
    None,
    /// Re-run a create job with the mode the user picked.
    Create(u64, Box<Repository>, Box<CreateRequest>),
    RunScript { job: u64, repo_id: String, worktree_path: String },
}

#[derive(Debug, Default)]
pub struct Jobs {
    pub list: Vec<Job>,
    next_id: u64,
    /// Keys go to the pane (Tab toggles).
    pub focused: bool,
    /// Selected job within the pane.
    pub cursor: usize,
}

impl Jobs {
    fn push(&mut self, kind: JobKind, label: String) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.list.push(Job { id, kind, label, steps: Vec::new(), log: Vec::new(), question: None, state: JobState::Running, finished_at: None });
        id
    }

    pub fn start_create(&mut self, repo: Repository, req: CreateRequest) -> u64 {
        let label = format!("Create {}", req.branch);
        self.push(JobKind::Create { repo, req, worktree_id: None }, label)
    }

    pub fn start_delete(&mut self, worktree_id: String, name: &str) -> u64 {
        self.push(JobKind::Delete { worktree_id }, format!("Delete {name}"))
    }

    pub fn get(&self, id: u64) -> Option<&Job> {
        self.list.iter().find(|j| j.id == id)
    }

    fn get_mut(&mut self, id: u64) -> Option<&mut Job> {
        self.list.iter_mut().find(|j| j.id == id)
    }

    /// A new stage started; the previous one is done.
    pub fn step(&mut self, id: u64, text: String) {
        if let Some(j) = self.get_mut(id) {
            if let Some(last) = j.steps.last_mut() {
                last.done = true;
            }
            j.steps.push(Step { text, done: false });
        }
    }

    pub fn log(&mut self, id: u64, line: String) {
        if let Some(j) = self.get_mut(id) {
            j.log.push(line);
            if j.log.len() > LOG_CAP {
                j.log.remove(0);
            }
        }
    }

    pub fn ask(&mut self, id: u64, q: Question) {
        if let Some(j) = self.get_mut(id) {
            if let Some(last) = j.steps.last_mut() {
                last.done = true;
            }
            j.question = Some(q);
            j.state = JobState::Waiting;
        }
    }

    /// The created worktree's id, for the placeholder row and selection.
    pub fn set_worktree(&mut self, id: u64, wt_id: String) {
        if let Some(Job { kind: JobKind::Create { worktree_id, .. }, .. }) = self.get_mut(id) {
            *worktree_id = Some(wt_id);
        }
    }

    pub fn finish(&mut self, id: u64, result: Result<(), String>, now: Instant) {
        if let Some(j) = self.get_mut(id) {
            // A failure leaves the stage it failed in open (shown as ✗).
            if let Some(last) = j.steps.last_mut()
                && result.is_ok()
            {
                last.done = true;
            }
            j.question = None;
            j.state = match result {
                Ok(()) => JobState::Done,
                Err(e) => JobState::Failed(e),
            };
            j.finished_at = Some(now);
        }
    }

    pub fn dismiss(&mut self, id: u64) {
        self.list.retain(|j| j.id != id);
        self.clamp();
    }

    fn clamp(&mut self) {
        self.cursor = self.cursor.min(self.list.len().saturating_sub(1));
        if self.list.is_empty() {
            self.focused = false;
        }
    }

    /// Drop finished jobs past their linger time. Returns whether anything changed.
    pub fn tick(&mut self, now: Instant) -> bool {
        let before = self.list.len();
        self.list.retain(|j| {
            let linger = if j.log.is_empty() { DONE_LINGER } else { DONE_LINGER_LOG };
            !(j.state == JobState::Done && j.finished_at.is_some_and(|t| now.duration_since(t) >= linger))
        });
        self.clamp();
        self.list.len() != before
    }

    pub fn visible(&self) -> bool {
        !self.list.is_empty()
    }

    pub fn waiting(&self) -> bool {
        self.list.iter().any(|j| j.question.is_some())
    }

    /// Focus the pane, on the first job with a question if there is one.
    pub fn focus(&mut self) {
        if self.list.is_empty() {
            return;
        }
        self.focused = true;
        if let Some(i) = self.list.iter().position(|j| j.question.is_some()) {
            self.cursor = i;
        }
        self.clamp();
    }

    pub fn on_key(&mut self, key: Key) -> JobEffect {
        match key {
            Key::Tab | Key::Esc => {
                self.focused = false;
                return JobEffect::None;
            }
            Key::Down | Key::Char('j') => {
                self.cursor = (self.cursor + 1).min(self.list.len().saturating_sub(1));
                return JobEffect::None;
            }
            Key::Up | Key::Char('k') => {
                self.cursor = self.cursor.saturating_sub(1);
                return JobEffect::None;
            }
            _ => {}
        }
        let Some(job) = self.list.get_mut(self.cursor) else { return JobEffect::None };
        let id = job.id;
        match (&job.question, &job.kind) {
            (Some(Question::BranchExists { local, remote }), JobKind::Create { repo, req, .. }) => {
                let mode = match (key, *remote) {
                    (Key::Char('n'), _) => {
                        self.dismiss(id);
                        return JobEffect::None;
                    }
                    (Key::Enter | Key::Char('p'), true) => CreateMode::TrackRemote,
                    (Key::Char('c'), true) => CreateMode::Disconnected,
                    (Key::Enter | Key::Char('r'), false) if *local => CreateMode::ReuseLocal,
                    (Key::Char('d'), false) if *local => CreateMode::DeleteAndRecreate,
                    _ => return JobEffect::None,
                };
                let req = CreateRequest { mode, ..req.clone() };
                let repo = repo.clone();
                job.kind = JobKind::Create { repo: repo.clone(), req: req.clone(), worktree_id: None };
                job.question = None;
                job.state = JobState::Running;
                // Focus back on the list: the question has been answered.
                self.focused = false;
                JobEffect::Create(id, Box::new(repo), Box::new(req))
            }
            (Some(Question::RunScript { repo_id, worktree_path }), _) => match key {
                Key::Enter | Key::Char('y') => {
                    let effect = JobEffect::RunScript { job: id, repo_id: repo_id.clone(), worktree_path: worktree_path.clone() };
                    job.question = None;
                    job.state = JobState::Running;
                    self.focused = false;
                    effect
                }
                Key::Char('n') => {
                    self.finish(id, Ok(()), Instant::now());
                    self.focused = false;
                    JobEffect::None
                }
                _ => JobEffect::None,
            },
            _ if key == Key::Char('x') && job.state != JobState::Running => {
                self.dismiss(id);
                JobEffect::None
            }
            _ => JobEffect::None,
        }
    }
}

/// Strip ANSI escape sequences (CSI and OSC) from script output.
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            if c != '\r' {
                out.push(c);
            }
            continue;
        }
        match chars.next() {
            Some('[') => {
                // CSI: parameters, then one final byte in @..~.
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
            Some(']') => {
                // OSC: up to BEL or ESC \.
                while let Some(c) = chars.next() {
                    if c == '\x07' {
                        break;
                    }
                    if c == '\x1b' {
                        chars.next();
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> Repository {
        Repository { id: "r".into(), path: "/r".into(), name: "r".into() }
    }

    fn req() -> CreateRequest {
        CreateRequest { branch: "feat".into(), base_branch: None, custom_name: None, mode: CreateMode::Fresh, tmp_space: false }
    }

    #[test]
    fn branch_exists_choices_rerun_the_same_job() {
        let mut jobs = Jobs::default();
        let id = jobs.start_create(repo(), req());
        jobs.ask(id, Question::BranchExists { local: true, remote: true });
        jobs.focus();
        assert!(jobs.focused);
        assert_eq!(jobs.on_key(Key::Char('r')), JobEffect::None, "reuse needs a local-only branch");
        match jobs.on_key(Key::Enter) {
            JobEffect::Create(jid, _, r) => {
                assert_eq!(jid, id);
                assert_eq!(r.mode, CreateMode::TrackRemote);
            }
            e => panic!("{e:?}"),
        }
        assert!(!jobs.focused);
        assert_eq!(jobs.get(id).unwrap().state, JobState::Running);
        assert_eq!(jobs.list.len(), 1);

        jobs.ask(id, Question::BranchExists { local: true, remote: false });
        jobs.focus();
        assert!(matches!(jobs.on_key(Key::Char('d')), JobEffect::Create(_, _, r) if r.mode == CreateMode::DeleteAndRecreate));
        jobs.ask(id, Question::BranchExists { local: true, remote: true });
        jobs.focus();
        assert!(matches!(jobs.on_key(Key::Char('c')), JobEffect::Create(_, _, r) if r.mode == CreateMode::Disconnected));
        jobs.ask(id, Question::BranchExists { local: true, remote: false });
        jobs.focus();
        assert_eq!(jobs.on_key(Key::Char('n')), JobEffect::None);
        assert!(!jobs.visible(), "going back drops the job");
    }

    #[test]
    fn run_script_question() {
        let mut jobs = Jobs::default();
        let id = jobs.start_create(repo(), req());
        jobs.set_worktree(id, "w1".into());
        jobs.ask(id, Question::RunScript { repo_id: "r".into(), worktree_path: "/r/w".into() });
        assert!(jobs.waiting());
        jobs.focus();
        assert_eq!(
            jobs.on_key(Key::Char('y')),
            JobEffect::RunScript { job: id, repo_id: "r".into(), worktree_path: "/r/w".into() }
        );
        assert_eq!(jobs.get(id).unwrap().state, JobState::Running);

        let id2 = jobs.start_create(repo(), req());
        jobs.ask(id2, Question::RunScript { repo_id: "r".into(), worktree_path: "/r/w".into() });
        jobs.focus();
        assert_eq!(jobs.cursor, 1, "focus jumps to the job with a question");
        jobs.on_key(Key::Char('n'));
        assert_eq!(jobs.get(id2).unwrap().state, JobState::Done);
    }

    #[test]
    fn steps_finish_and_linger() {
        let mut jobs = Jobs::default();
        let id = jobs.start_create(repo(), req());
        assert!(jobs.get(id).unwrap().pending_create().is_some());
        jobs.step(id, "Fetching…".into());
        jobs.step(id, "Adding worktree…".into());
        let j = jobs.get(id).unwrap();
        assert!(j.steps[0].done && !j.steps[1].done);
        assert_eq!(j.current_step(), Some("Adding worktree…"));

        let t0 = Instant::now();
        jobs.set_worktree(id, "w1".into());
        jobs.finish(id, Ok(()), t0);
        assert_eq!(jobs.get(id).unwrap().pending_create().unwrap().2, Some("w1"));
        assert!(!jobs.tick(t0 + Duration::from_secs(1)));
        assert!(jobs.visible());
        assert!(jobs.tick(t0 + DONE_LINGER));
        assert!(!jobs.visible());
    }

    #[test]
    fn failures_stay_until_dismissed() {
        let mut jobs = Jobs::default();
        let id = jobs.start_delete("w1".into(), "feat");
        let t0 = Instant::now();
        jobs.finish(id, Err("boom".into()), t0);
        assert!(!jobs.tick(t0 + Duration::from_secs(60)));
        jobs.focus();
        jobs.on_key(Key::Char('x'));
        assert!(!jobs.visible());
        assert!(!jobs.focused);
    }

    #[test]
    fn running_jobs_cannot_be_dismissed() {
        let mut jobs = Jobs::default();
        jobs.start_delete("w1".into(), "feat");
        jobs.focus();
        jobs.on_key(Key::Char('x'));
        assert!(jobs.visible());
    }

    #[test]
    fn strips_ansi() {
        assert_eq!(strip_ansi("\x1b[1;32mok\x1b[0m done\r"), "ok done");
        assert_eq!(strip_ansi("\x1b]0;title\x07x"), "x");
    }
}
