//! Worktree lifecycle dialogs: new worktree form (with the advisory live
//! "does this branch exist?" check), branch-exists choices, delete
//! confirmation, startup-script prompt, and a busy indicator. Pure state +
//! key handling; the work itself runs in ops.rs.

use std::time::{Duration, Instant};

use am_core::model::{Repository, WorktreeView};
use am_core::worktrees::{BranchCheck, CreateMode, CreateRequest};

use crate::app::Key;

/// Debounce for the live branch check while typing (same as the TS form).
pub const CHECK_DEBOUNCE: Duration = Duration::from_millis(400);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormField {
    Repo,
    Branch,
    Name,
    Base,
}

#[derive(Debug, Clone)]
pub struct NewForm {
    pub repos: Vec<Repository>,
    pub repo: usize,
    pub branch: String,
    pub name: String,
    pub base: String,
    pub field: FormField,
    /// Last answer from the live check, for the branch it was asked about.
    pub check: Option<(String, BranchCheck)>,
    /// Branch typed at this instant, not yet checked.
    pub pending_check: Option<(String, Instant)>,
    pub error: Option<String>,
}

impl NewForm {
    pub fn repo(&self) -> &Repository {
        &self.repos[self.repo]
    }

    fn fields(&self) -> Vec<FormField> {
        let mut f = Vec::new();
        if self.repos.len() > 1 {
            f.push(FormField::Repo);
        }
        f.push(FormField::Branch);
        f.push(FormField::Name);
        // Tracking origin/<branch>: the base doesn't apply.
        if !self.remote_exists() {
            f.push(FormField::Base);
        }
        f
    }

    pub fn remote_exists(&self) -> bool {
        self.check.as_ref().is_some_and(|(b, c)| *b == self.branch && c.remote == Some(true))
    }

    pub fn local_only(&self) -> bool {
        self.check.as_ref().is_some_and(|(b, c)| *b == self.branch && c.local && c.remote != Some(true))
    }

    fn buffer(&mut self) -> Option<&mut String> {
        match self.field {
            FormField::Branch => Some(&mut self.branch),
            FormField::Name => Some(&mut self.name),
            FormField::Base => Some(&mut self.base),
            FormField::Repo => None,
        }
    }

    fn typed(&mut self) {
        self.error = None;
        if self.field == FormField::Branch {
            self.pending_check = (!self.branch.trim().is_empty()).then(|| (self.branch.clone(), Instant::now()));
        }
    }

    fn request(&self) -> CreateRequest {
        let opt = |s: &str| Some(s.trim().to_string()).filter(|s| !s.is_empty());
        CreateRequest { branch: self.branch.trim().to_string(), base_branch: opt(&self.base), custom_name: opt(&self.name), mode: CreateMode::Fresh }
    }
}

#[derive(Debug, Clone)]
pub enum DeleteStep {
    Confirm,
    LocalBranch,
}

#[derive(Debug, Clone)]
pub struct DeleteState {
    pub view: WorktreeView,
    pub step: DeleteStep,
    pub branch_only: bool,
    pub stale: bool,
}

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)] // one short-lived modal at a time
pub enum Modal {
    New(NewForm),
    BranchExists { repo: Repository, req: CreateRequest, local: bool, remote: bool },
    Delete(DeleteState),
    RunScript { repo_id: String, worktree_path: String },
    Busy(String),
}

pub enum Effect {
    None,
    Close,
    Create(Repository, CreateRequest),
    Delete { view: Box<WorktreeView>, delete_branch: bool },
    RunScript { repo_id: String, worktree_path: String },
}

impl Modal {
    pub fn new_worktree(repos: Vec<Repository>, default_repo: Option<&str>, prefix: &str, base: &str) -> Modal {
        let repo = default_repo.and_then(|id| repos.iter().position(|r| r.id == id)).unwrap_or(0);
        Modal::New(NewForm {
            repos,
            repo,
            branch: prefix.to_string(),
            name: String::new(),
            base: base.to_string(),
            field: FormField::Branch,
            check: None,
            pending_check: None,
            error: None,
        })
    }

    pub fn delete(view: WorktreeView) -> Modal {
        let exists = std::path::Path::new(&view.worktree.path).exists();
        let w = &view.worktree;
        let branch_only = (w.is_main && w.branch != "main" && w.branch != "master") || (!exists && !w.is_main);
        let stale = !exists && !w.is_main;
        Modal::Delete(DeleteState { view, step: DeleteStep::Confirm, branch_only, stale })
    }

    pub fn on_key(&mut self, key: Key) -> Effect {
        match self {
            Modal::Busy(_) => Effect::None,
            Modal::New(f) => {
                let fields = f.fields();
                let idx = fields.iter().position(|x| *x == f.field).unwrap_or(0);
                match key {
                    Key::Esc => return Effect::Close,
                    Key::Tab | Key::Down => f.field = fields[(idx + 1) % fields.len()],
                    Key::BackTab | Key::Up => f.field = fields[(idx + fields.len() - 1) % fields.len()],
                    Key::Left | Key::Right if f.field == FormField::Repo => {
                        let n = f.repos.len();
                        f.repo = if key == Key::Right { (f.repo + 1) % n } else { (f.repo + n - 1) % n };
                        f.check = None;
                        f.typed();
                    }
                    Key::Backspace => {
                        if let Some(b) = f.buffer() {
                            b.pop();
                        }
                        f.typed();
                    }
                    Key::Char(c) => {
                        if f.field == FormField::Branch && c.is_whitespace() {
                            return Effect::None; // branch names can't contain spaces
                        }
                        if let Some(b) = f.buffer() {
                            b.push(c);
                        }
                        f.typed();
                    }
                    Key::Enter => {
                        if f.branch.trim().is_empty() || f.branch.ends_with('/') {
                            f.error = Some("Enter a branch name".into());
                            f.field = FormField::Branch;
                            return Effect::None;
                        }
                        return Effect::Create(f.repo().clone(), f.request());
                    }
                    _ => {}
                }
                if !f.fields().contains(&f.field) {
                    f.field = FormField::Branch;
                }
                Effect::None
            }
            Modal::BranchExists { repo, req, local, remote } => {
                let mode = match (key, *remote) {
                    (Key::Esc | Key::Char('n'), _) => return Effect::Close,
                    (Key::Enter | Key::Char('p'), true) => CreateMode::TrackRemote,
                    (Key::Char('c'), true) => CreateMode::Disconnected,
                    (Key::Enter | Key::Char('r'), false) if *local => CreateMode::ReuseLocal,
                    (Key::Char('d'), false) if *local => CreateMode::DeleteAndRecreate,
                    _ => return Effect::None,
                };
                Effect::Create(repo.clone(), CreateRequest { mode, ..req.clone() })
            }
            Modal::Delete(d) => match (&d.step, key) {
                (_, Key::Esc) | (DeleteStep::Confirm, Key::Char('n')) => Effect::Close,
                (DeleteStep::Confirm, Key::Enter | Key::Char('y')) => {
                    if d.branch_only {
                        Effect::Delete { view: Box::new(d.view.clone()), delete_branch: true }
                    } else {
                        d.step = DeleteStep::LocalBranch;
                        Effect::None
                    }
                }
                (DeleteStep::LocalBranch, Key::Enter | Key::Char('y')) => {
                    Effect::Delete { view: Box::new(d.view.clone()), delete_branch: true }
                }
                (DeleteStep::LocalBranch, Key::Char('n')) => Effect::Delete { view: Box::new(d.view.clone()), delete_branch: false },
                _ => Effect::None,
            },
            Modal::RunScript { repo_id, worktree_path, .. } => match key {
                Key::Enter | Key::Char('y') => Effect::RunScript { repo_id: repo_id.clone(), worktree_path: worktree_path.clone() },
                Key::Esc | Key::Char('n') => Effect::Close,
                _ => Effect::None,
            },
        }
    }

    /// The live check that is due now (debounced), if any.
    pub fn due_check(&mut self) -> Option<(String, String)> {
        let Modal::New(f) = self else { return None };
        let (branch, at) = f.pending_check.as_ref()?;
        if at.elapsed() < CHECK_DEBOUNCE || branch.ends_with('/') {
            return None;
        }
        let branch = branch.clone();
        f.pending_check = None;
        Some((f.repo().path.clone(), branch))
    }

    pub fn on_check_result(&mut self, branch: String, result: BranchCheck) {
        if let Modal::New(f) = self
            && f.branch == branch
        {
            f.check = Some((branch, result));
            if !f.fields().contains(&f.field) {
                f.field = FormField::Branch;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repos() -> Vec<Repository> {
        vec![
            Repository { id: "r1".into(), path: "/r1".into(), name: "one".into() },
            Repository { id: "r2".into(), path: "/r2".into(), name: "two".into() },
        ]
    }

    fn typed(m: &mut Modal, s: &str) {
        for c in s.chars() {
            m.on_key(Key::Char(c));
        }
    }

    #[test]
    fn new_form_builds_a_fresh_request() {
        let mut m = Modal::new_worktree(repos(), Some("r2"), "feature/", "main");
        typed(&mut m, "x y");
        m.on_key(Key::Tab);
        typed(&mut m, "Nick");
        let Effect::Create(repo, req) = m.on_key(Key::Enter) else { panic!() };
        assert_eq!(repo.id, "r2");
        assert_eq!(req.branch, "feature/xy", "spaces are dropped");
        assert_eq!(req.custom_name.as_deref(), Some("Nick"));
        assert_eq!(req.base_branch.as_deref(), Some("main"));
        assert_eq!(req.mode, CreateMode::Fresh);
    }

    #[test]
    fn empty_branch_is_refused() {
        let mut m = Modal::new_worktree(repos(), None, "feature/", "main");
        assert!(matches!(m.on_key(Key::Enter), Effect::None));
        let Modal::New(f) = &m else { panic!() };
        assert!(f.error.is_some());
    }

    #[test]
    fn live_check_is_debounced_and_hides_base_when_on_origin() {
        let mut m = Modal::new_worktree(repos(), None, "", "main");
        typed(&mut m, "feat");
        assert!(m.due_check().is_none(), "not before the debounce");
        if let Modal::New(f) = &mut m {
            f.pending_check.as_mut().unwrap().1 -= CHECK_DEBOUNCE;
        }
        assert_eq!(m.due_check(), Some(("/r1".into(), "feat".into())));
        m.on_check_result("feat".into(), BranchCheck { local: false, remote: Some(true) });
        let Modal::New(f) = &m else { panic!() };
        assert!(f.remote_exists());
        assert!(!f.fields().contains(&FormField::Base));
        // A stale answer for an older spelling is ignored.
        m.on_check_result("fe".into(), BranchCheck { local: true, remote: Some(false) });
        let Modal::New(f) = &m else { panic!() };
        assert!(f.remote_exists());
    }

    #[test]
    fn branch_exists_choices_match_ts() {
        let req = CreateRequest { branch: "b".into(), ..Default::default() };
        let r = repos()[0].clone();
        let mut remote = Modal::BranchExists { repo: r.clone(), req: req.clone(), local: true, remote: true };
        assert!(matches!(remote.on_key(Key::Enter), Effect::Create(_, CreateRequest { mode: CreateMode::TrackRemote, .. })));
        assert!(matches!(remote.on_key(Key::Char('c')), Effect::Create(_, CreateRequest { mode: CreateMode::Disconnected, .. })));
        assert!(matches!(remote.on_key(Key::Char('r')), Effect::None));
        let mut local = Modal::BranchExists { repo: r, req, local: true, remote: false };
        assert!(matches!(local.on_key(Key::Enter), Effect::Create(_, CreateRequest { mode: CreateMode::ReuseLocal, .. })));
        assert!(matches!(local.on_key(Key::Char('d')), Effect::Create(_, CreateRequest { mode: CreateMode::DeleteAndRecreate, .. })));
        assert!(matches!(local.on_key(Key::Esc), Effect::Close));
    }
}
