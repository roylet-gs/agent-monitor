//! Runs the open actions off the UI thread and reports back as messages.
//! Port of `handleOpen` / `handleOpenClaude` in `src/app.tsx`, generalised to
//! any agent CLI (`am_core::harness`).

use std::path::Path;
use std::sync::mpsc::Sender;
use std::time::Duration;

use am_core::grouping;
use am_core::launch::{self, IdeChoice, OpenAction};
use am_core::model::WorktreeView;
use am_core::settings::Settings;

pub enum Msg {
    /// Show a centered popup.
    Popup(String),
    /// Short status-bar notice.
    Notice(String),
    BranchChecked(String, am_core::worktrees::BranchCheck),
    /// A job (jobs pane) started a new stage.
    JobStep(u64, String),
    /// A line of startup-script output for a job.
    JobLog(u64, String),
    /// Job id, and the created worktree or an error.
    Created(u64, Result<am_core::model::Worktree, am_core::worktrees::CreateError>),
    /// Job id, worktree id, label, and warnings or an error.
    Deleted(u64, String, String, Result<Vec<String>, String>),
    /// Startup script finished (or was handed to a new terminal window).
    ScriptDone(u64, Result<(), String>),
    /// Typed path, and the repo name or an error.
    RepoAdded(String, Result<String, String>),
    /// A newer release exists.
    UpdateAvailable(am_core::update_check::Update),
}

/// Long enough for the popup to register before the editor steals focus.
const POPUP_BEFORE_OPEN: Duration = Duration::from_millis(600);

/// The agent the hooks last saw a session from, if any.
fn known_agent(v: &WorktreeView) -> Option<am_core::harness::HarnessKind> {
    v.agent.as_ref().filter(|a| a.session_id.is_some()).map(|a| a.agent)
}

pub fn open(v: WorktreeView, settings: &Settings, tx: Sender<Msg>) {
    let ide = IdeChoice::from_setting(&settings.ide);
    let resume_on = settings.resume_last_session;
    let default_agent = settings.default_agent();
    std::thread::spawn(move || {
        let path = Path::new(&v.worktree.path);
        let label = v.worktree.display_name().to_string();
        // A terminal is already open there: bring it forward rather than open another.
        if ide == IdeChoice::Terminal && v.has_terminal {
            report(&tx, launch::focus_terminal_app(), format!("Focused terminal for {label}"));
            return;
        }
        let already_open = grouping::is_open_now(v.agent.as_ref());
        let resume = if resume_on && !already_open {
            launch::resume_for(path, known_agent(&v), default_agent)
        } else {
            Default::default()
        };
        let result = match launch::resolve_open_action(resume_on, already_open, ide, resume) {
            OpenAction::Plain => launch::open_in_editor(path, ide),
            OpenAction::TerminalAgent(r) => launch::open_terminal(path, Some(&launch::agent_command(&r))),
            OpenAction::CopyAndOpen(r) => {
                let cmd = launch::agent_command(&r);
                let msg = if launch::copy_to_clipboard(&cmd) {
                    format!("✓ Copied to clipboard — paste in the editor's terminal to resume:\n\n{cmd}\n\nTurn off with the resumeLastSession setting.")
                } else {
                    format!("Couldn't copy. Run this in the editor's terminal:\n\n{cmd}")
                };
                let _ = tx.send(Msg::Popup(msg));
                std::thread::sleep(POPUP_BEFORE_OPEN);
                launch::open_in_editor(path, ide)
            }
        };
        report(&tx, result, format!("Opened {label}"));
    });
}

pub fn open_terminal(v: WorktreeView, tx: Sender<Msg>) {
    std::thread::spawn(move || {
        let label = v.worktree.display_name().to_string();
        let r = if v.has_terminal { launch::focus_terminal_app() } else { launch::open_terminal(Path::new(&v.worktree.path), None) };
        report(&tx, r, format!("Terminal for {label}"));
    });
}

pub fn open_agent(v: WorktreeView, settings: &Settings, tx: Sender<Msg>) {
    let default_agent = settings.default_agent();
    std::thread::spawn(move || {
        let path = Path::new(&v.worktree.path);
        let cmd = launch::agent_command(&launch::resume_for(path, known_agent(&v), default_agent));
        report(&tx, launch::open_terminal(path, Some(&cmd)), format!("Started `{cmd}`"));
    });
}

fn report(tx: &Sender<Msg>, r: Result<(), String>, ok: String) {
    let _ = tx.send(match r {
        Ok(()) => Msg::Notice(ok),
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
        report(&tx, r, format!("Opened {url}"));
    });
}

/// Checks for a newer release now and then every [`update_check::CHECK_INTERVAL`].
/// Re-reads `checkForUpdates` each time, so turning it off stops the requests.
pub fn spawn_update_check(tx: Sender<Msg>) {
    use am_core::update_check;
    let spawned = std::thread::Builder::new().name("update-check".into()).spawn(move || {
        let method = update_check::InstallMethod::current();
        am_core::debug!("update", "install method: {method:?}");
        loop {
            if am_core::settings::load().check_for_updates
                && let Some(latest) = update_check::check(env!("CARGO_PKG_VERSION"))
                && tx.send(Msg::UpdateAvailable(update_check::Update::new(latest, method))).is_err()
            {
                return;
            }
            std::thread::sleep(update_check::CHECK_INTERVAL);
        }
    });
    if let Err(e) = spawned {
        am_core::debug!("update", "could not spawn update-check thread: {e}");
    }
}
