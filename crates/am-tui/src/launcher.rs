//! Runs the open actions off the UI thread and reports back as messages.
//! Port of `handleOpen` / `handleOpenClaude` in `src/app.tsx`.

use std::path::Path;
use std::sync::mpsc::Sender;
use std::time::Duration;

use am_core::grouping;
use am_core::launch::{self, IdeChoice, OpenAction, TerminalTarget};
use am_core::terminal::Backend;
use am_core::model::WorktreeView;
use am_core::settings::Settings;

pub enum Msg {
    /// Show a centered popup.
    Popup(String),
    /// Short status-bar notice.
    Notice(String),
    /// A worktree was opened / focused in a terminal or editor (notice text).
    Opened(String),
    BranchChecked(String, am_core::worktrees::BranchCheck),
    Created(
        Box<am_core::model::Repository>,
        Box<am_core::worktrees::CreateRequest>,
        Result<am_core::model::Worktree, am_core::worktrees::CreateError>,
    ),
    /// Worktree id, label, and warnings or an error.
    Deleted(String, String, Result<Vec<String>, String>),
    /// Typed path, and the repo name or an error.
    RepoAdded(String, Result<String, String>),
}

/// Long enough for the popup to register before the editor steals focus.
const POPUP_BEFORE_OPEN: Duration = Duration::from_millis(600);

pub fn open(v: WorktreeView, settings: &Settings, tx: Sender<Msg>) {
    let ide = IdeChoice::from_setting(&settings.ide);
    let target = TerminalTarget::from_settings(settings);
    let resume_on = settings.resume_last_session;
    std::thread::spawn(move || {
        let path = Path::new(&v.worktree.path);
        let label = v.worktree.display_name().to_string();
        // A terminal is already open there: bring it forward rather than open
        // another (when the backend can't find it, the app at least comes up).
        if ide == IdeChoice::Terminal && v.has_terminal && focus_existing(path, target, &tx, &label) {
            return;
        }
        // A background session runs here: attach to it rather than resume.
        if let Some(bg) = &v.background {
            let cmd = launch::attach_command(&bg.id);
            let result = if ide == IdeChoice::Terminal {
                launch::open_terminal(path, Some(&cmd), target)
            } else {
                copy_then_open(&cmd, "", path, ide, target, &tx)
            };
            report(&tx, result, format!("Attached to {label}"));
            return;
        }
        let already_open = grouping::is_open_now(v.agent.as_ref());
        let known = v.agent.as_ref().is_some_and(|a| a.session_id.is_some());
        let resume = if resume_on && !already_open { launch::resume_for(path, known) } else { Default::default() };
        let result = match launch::resolve_open_action(resume_on, already_open, ide, resume) {
            OpenAction::Plain => launch::open_in_editor(path, ide, target),
            OpenAction::TerminalClaude(r) => launch::open_terminal(path, Some(&launch::claude_command(&r)), target),
            OpenAction::CopyAndOpen(r) => copy_then_open(
                &launch::claude_command(&r),
                "\n\nTurn off with the resumeLastSession setting.",
                path,
                ide,
                target,
                &tx,
            ),
        };
        report(&tx, result, format!("Opened {label}"));
    });
}

/// Editor mode: copy `cmd` for the editor's terminal, say so, then open.
fn copy_then_open(
    cmd: &str,
    footer: &str,
    path: &Path,
    ide: IdeChoice,
    target: TerminalTarget,
    tx: &Sender<Msg>,
) -> Result<(), String> {
    let msg = if launch::copy_to_clipboard(cmd) {
        format!("✓ Copied to clipboard — paste in the editor's terminal to resume:\n\n{cmd}{footer}")
    } else {
        format!("Couldn't copy. Run this in the editor's terminal:\n\n{cmd}")
    };
    let _ = tx.send(Msg::Popup(msg));
    std::thread::sleep(POPUP_BEFORE_OPEN);
    launch::open_in_editor(path, ide, target)
}

/// Focus the terminal already at `path`. True when handled (focused, or the
/// app was brought up for a backend that can't list its tabs); false when the
/// backend looked and found nothing, so the caller should open a new one.
fn focus_existing(path: &Path, target: TerminalTarget, tx: &Sender<Msg>, label: &str) -> bool {
    match launch::focus_terminal(path, target) {
        Ok(true) => {
            report(tx, Ok(()), format!("Focused terminal for {label}"));
            true
        }
        Ok(false) if !matches!(target.backend, Backend::Tmux | Backend::Ghostty) => {
            report(tx, Ok(()), format!("Focused {}", target.backend.name()));
            true
        }
        Ok(false) => false,
        Err(e) => {
            report(tx, Err(e), String::new());
            true
        }
    }
}

pub fn open_terminal(v: WorktreeView, settings: &Settings, tx: Sender<Msg>) {
    let target = TerminalTarget::from_settings(settings);
    std::thread::spawn(move || {
        let label = v.worktree.display_name().to_string();
        let path = Path::new(&v.worktree.path);
        if v.has_terminal && focus_existing(path, target, &tx, &label) {
            return;
        }
        report(&tx, launch::open_terminal(path, None, target), format!("Terminal for {label}"));
    });
}

pub fn open_claude(v: WorktreeView, settings: &Settings, tx: Sender<Msg>) {
    let target = TerminalTarget::from_settings(settings);
    std::thread::spawn(move || {
        let path = Path::new(&v.worktree.path);
        let cmd = match &v.background {
            Some(bg) => launch::attach_command(&bg.id),
            None => {
                let known = v.agent.as_ref().is_some_and(|a| a.session_id.is_some());
                launch::claude_command(&launch::resume_for(path, known))
            }
        };
        report(&tx, launch::open_terminal(path, Some(&cmd), target), format!("Started `{cmd}`"));
    });
}

fn report(tx: &Sender<Msg>, r: Result<(), String>, ok: String) {
    let _ = tx.send(match r {
        Ok(()) => Msg::Opened(ok),
        Err(e) => Msg::Popup(format!("✗ {e}")),
    });
}

pub fn open_url(url: String, tx: Sender<Msg>) {
    std::thread::spawn(move || {
        let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
        let r = std::process::Command::new(opener)
            .arg(&url)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|e| e.to_string())
            .and_then(|s| if s.success() { Ok(()) } else { Err(format!("{opener} failed")) });
        let _ = tx.send(match r {
            Ok(()) => Msg::Notice(format!("Opened {url}")),
            Err(e) => Msg::Popup(format!("✗ {e}")),
        });
    });
}
