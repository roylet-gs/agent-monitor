//! Add repository (`A`, or `a` in Settings → Repositories…): browse
//! directories to a git repo, the same as the 1.x `FolderBrowser`. Repos are
//! listed first; typing filters by name, and typing a path (`/…` or `~…`)
//! jumps to it instead (Tab completes). Reads directories, but never touches
//! the DB: an add goes back to the app as `BrowserOutcome::Add`.

use std::path::{Path, PathBuf};

use crate::app::Key;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_repo: bool,
    /// Already a tracked repository.
    pub tracked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserOutcome {
    Stay,
    Cancel,
    /// Add the repository at this path.
    Add(String),
}

#[derive(Debug, Clone)]
pub struct FolderBrowser {
    pub dir: PathBuf,
    /// Subdirectories of `dir` (no `..`), repos first.
    entries: Vec<Entry>,
    pub filter: String,
    pub cursor: usize,
    pub error: Option<String>,
    /// Path being added (the add runs in the background).
    pub adding: Option<String>,
    tracked: Vec<String>,
}

/// Where the browser opens: next to the most recently used repo, else the
/// parent of the working directory, else home.
pub fn start_dir(repo_paths: &[String]) -> PathBuf {
    let parent = |p: &Path| p.parent().filter(|d| d.is_dir()).map(Path::to_path_buf);
    repo_paths
        .first()
        .and_then(|p| parent(Path::new(p)))
        .or_else(|| std::env::current_dir().ok().filter(|d| d.parent().is_some()).and_then(|d| parent(&d)))
        .unwrap_or_else(am_core::paths::home_dir)
}

fn expand(path: &str) -> PathBuf {
    match path.strip_prefix('~') {
        Some(rest) => PathBuf::from(format!("{}{rest}", am_core::paths::home_dir().display())),
        None => PathBuf::from(path),
    }
}

impl FolderBrowser {
    /// `tracked`: paths of the repositories already tracked.
    pub fn open(dir: PathBuf, tracked: Vec<String>) -> FolderBrowser {
        let mut b = FolderBrowser { dir: PathBuf::new(), entries: Vec::new(), filter: String::new(), cursor: 0, error: None, adding: None, tracked };
        b.go(dir);
        b
    }

    fn go(&mut self, dir: PathBuf) {
        self.filter.clear();
        self.cursor = 0;
        self.entries.clear();
        match std::fs::read_dir(&dir) {
            Ok(rd) => {
                let mut entries: Vec<Entry> = rd
                    .flatten()
                    .filter_map(|e| {
                        let name = e.file_name().to_string_lossy().into_owned();
                        let path = e.path();
                        (!name.starts_with('.') && path.is_dir()).then(|| {
                            let is_repo = am_core::worktrees::is_git_repo(&path);
                            let tracked = is_repo && self.tracked.iter().any(|t| Path::new(t) == path);
                            Entry { name, path, is_repo, tracked }
                        })
                    })
                    .collect();
                entries.sort_by(|a, b| b.is_repo.cmp(&a.is_repo).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
                am_core::debug!("repo", "Browsing {} ({} dirs)", dir.display(), entries.len());
                self.entries = entries;
            }
            Err(e) => self.error = Some(format!("Can't read {}: {e}", dir.display())),
        }
        self.dir = dir;
    }

    /// Typed text is a path (`/…`, `~…`) rather than a name filter.
    pub fn path_mode(&self) -> bool {
        self.filter.starts_with('/') || self.filter.starts_with('~')
    }

    /// The rows shown: `..` (when there is a parent and no filter), then the
    /// entries matching the filter. Nothing in path mode.
    pub fn visible(&self) -> Vec<Entry> {
        if self.path_mode() {
            return Vec::new();
        }
        let mut out = Vec::new();
        if self.filter.is_empty()
            && let Some(parent) = self.dir.parent()
        {
            out.push(Entry { name: "..".into(), path: parent.to_path_buf(), is_repo: false, tracked: false });
        }
        let needle = self.filter.to_lowercase();
        out.extend(self.entries.iter().filter(|e| e.name.to_lowercase().contains(&needle)).cloned());
        out
    }

    fn up(&mut self) {
        if let Some(parent) = self.dir.parent().map(Path::to_path_buf) {
            let from = self.dir.file_name().map(|n| n.to_string_lossy().into_owned());
            self.go(parent);
            // Land on the directory we came from.
            if let Some(from) = from {
                self.cursor = self.visible().iter().position(|e| e.name == from).unwrap_or(0);
            }
        }
    }

    fn add(&mut self, e: &Entry) -> BrowserOutcome {
        if e.tracked {
            self.error = Some(format!("{} is already tracked", e.name));
            return BrowserOutcome::Stay;
        }
        let path = e.path.display().to_string();
        self.adding = Some(path.clone());
        BrowserOutcome::Add(path)
    }

    pub fn on_key(&mut self, key: Key) -> BrowserOutcome {
        if self.adding.is_some() {
            return BrowserOutcome::Stay;
        }
        self.error = None;
        let rows = self.visible();
        let selected = rows.get(self.cursor).cloned();
        match key {
            Key::Esc => return BrowserOutcome::Cancel,
            Key::Up => self.cursor = self.cursor.saturating_sub(1),
            Key::Down => self.cursor = (self.cursor + 1).min(rows.len().saturating_sub(1)),
            Key::PageUp => self.cursor = self.cursor.saturating_sub(10),
            Key::PageDown => self.cursor = (self.cursor + 10).min(rows.len().saturating_sub(1)),
            Key::Left => self.up(),
            Key::Backspace if self.filter.is_empty() => self.up(),
            Key::Backspace => {
                self.filter.pop();
                self.cursor = 0;
            }
            Key::Tab if self.path_mode() => self.filter = crate::modals::complete_dir(&self.filter),
            Key::Right | Key::Tab => match selected {
                Some(e) if e.name == ".." => self.up(),
                Some(e) => self.go(e.path),
                None => {}
            },
            Key::Enter if self.path_mode() => {
                let typed = self.filter.trim().trim_end_matches('/').to_string();
                let path = expand(if typed.is_empty() { "/" } else { &typed });
                if am_core::worktrees::is_git_repo(&path) {
                    let tracked = self.tracked.iter().any(|t| Path::new(t) == path);
                    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    return self.add(&Entry { name, path, is_repo: true, tracked });
                }
                if path.is_dir() {
                    self.go(path);
                } else {
                    self.error = Some(format!("No such directory: {typed}"));
                }
            }
            Key::Enter => match selected {
                Some(e) if e.name == ".." => self.up(),
                Some(e) if e.is_repo => return self.add(&e),
                Some(e) => self.go(e.path),
                None => {}
            },
            Key::Char(c) => {
                self.filter.push(c);
                self.cursor = 0;
            }
            _ => {}
        }
        BrowserOutcome::Stay
    }

    /// A browser over given entries, without reading the disk (UI snapshots).
    #[cfg(test)]
    pub fn fixture(dir: PathBuf, entries: Vec<Entry>) -> FolderBrowser {
        FolderBrowser { dir, entries, filter: String::new(), cursor: 0, error: None, adding: None, tracked: vec![] }
    }

    /// The add failed: show why and take keys again.
    pub fn add_failed(&mut self, error: String) {
        self.adding = None;
        self.error = Some(error);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        for d in ["zeta-repo/.git", "alpha", "Beta", "mid-repo/.git", ".hidden", "alpha/inner-repo/.git"] {
            std::fs::create_dir_all(t.path().join(d)).unwrap();
        }
        std::fs::write(t.path().join("file.txt"), "x").unwrap();
        t
    }

    fn names(b: &FolderBrowser) -> Vec<String> {
        b.visible().into_iter().map(|e| e.name).collect()
    }

    #[test]
    fn lists_repos_first_then_dirs_without_dot_dirs_or_files() {
        let t = tree();
        let b = FolderBrowser::open(t.path().to_path_buf(), vec![]);
        assert_eq!(names(&b), vec!["..", "mid-repo", "zeta-repo", "alpha", "Beta"]);
        assert!(b.visible()[1].is_repo && !b.visible()[3].is_repo);
    }

    #[test]
    fn enter_adds_a_repo_and_enters_a_plain_dir() {
        let t = tree();
        let mut b = FolderBrowser::open(t.path().to_path_buf(), vec![]);
        b.cursor = 1;
        let want = t.path().join("mid-repo").display().to_string();
        assert_eq!(b.on_key(Key::Enter), BrowserOutcome::Add(want.clone()));
        assert_eq!(b.adding, Some(want));
        // Keys are ignored while the add runs; a failure hands them back.
        assert_eq!(b.on_key(Key::Esc), BrowserOutcome::Stay);
        b.add_failed("nope".into());
        assert_eq!(b.error.as_deref(), Some("nope"));

        b.cursor = 3;
        assert_eq!(b.on_key(Key::Enter), BrowserOutcome::Stay);
        assert_eq!(b.dir, t.path().join("alpha"));
        assert_eq!(names(&b), vec!["..", "inner-repo"]);
        // Going up lands on the directory we came from.
        b.on_key(Key::Left);
        assert_eq!(b.dir, t.path());
        assert_eq!(b.visible()[b.cursor].name, "alpha");
    }

    #[test]
    fn typing_filters_and_backspace_goes_up_when_empty() {
        let t = tree();
        let mut b = FolderBrowser::open(t.path().join("alpha"), vec![]);
        b.on_key(Key::Backspace);
        assert_eq!(b.dir, t.path());
        for c in "BET".chars() {
            b.on_key(Key::Char(c));
        }
        assert_eq!(names(&b), vec!["Beta"]);
        b.on_key(Key::Backspace);
        assert_eq!(b.filter, "BE");
        assert_eq!(b.dir, t.path());
    }

    #[test]
    fn tracked_repos_are_marked_and_refused() {
        let t = tree();
        let tracked = t.path().join("zeta-repo").display().to_string();
        let mut b = FolderBrowser::open(t.path().to_path_buf(), vec![tracked]);
        let rows = b.visible();
        assert!(rows[2].tracked && !rows[1].tracked);
        b.cursor = 2;
        assert_eq!(b.on_key(Key::Enter), BrowserOutcome::Stay);
        assert!(b.error.as_deref().is_some_and(|e| e.contains("already tracked")));
    }

    #[test]
    fn a_typed_path_jumps_or_adds() {
        let t = tree();
        let mut b = FolderBrowser::open(t.path().to_path_buf(), vec![]);
        for c in t.path().join("alpha").display().to_string().chars() {
            b.on_key(Key::Char(c));
        }
        assert!(b.path_mode() && b.visible().is_empty());
        assert_eq!(b.on_key(Key::Enter), BrowserOutcome::Stay);
        assert_eq!(b.dir, t.path().join("alpha"));
        assert!(b.filter.is_empty());

        let repo = t.path().join("zeta-repo");
        for c in format!("{}/", repo.display()).chars() {
            b.on_key(Key::Char(c));
        }
        assert_eq!(b.on_key(Key::Enter), BrowserOutcome::Add(repo.display().to_string()));

        let mut b = FolderBrowser::open(t.path().to_path_buf(), vec![]);
        for c in "/no/such/dir".chars() {
            b.on_key(Key::Char(c));
        }
        b.on_key(Key::Enter);
        assert!(b.error.as_deref().is_some_and(|e| e.contains("No such directory")));
    }
}
