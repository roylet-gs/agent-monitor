//! Runs the open actions off the UI thread and reports back as messages.
//! Port of `handleOpen` / `handleOpenClaude` in `src/app.tsx`.

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
}

/// Long enough for the popup to register before the editor steals focus.
const POPUP_BEFORE_OPEN: Duration = Duration::from_millis(600);

pub fn open(v: WorktreeView, settings: &Settings, tx: Sender<Msg>) {
    let ide = IdeChoice::from_setting(&settings.ide);
    let resume_on = settings.resume_last_session;
    std::thread::spawn(move || {
        let path = Path::new(&v.worktree.path);
        let label = v.worktree.display_name().to_string();
        // A terminal is already open there: bring it forward rather than open another.
        if ide == IdeChoice::Terminal && v.has_terminal {
            report(&tx, launch::focus_terminal_app(), format!("Focused terminal for {label}"));
            return;
        }
        let already_open = grouping::is_open_now(v.agent.as_ref());
        let known = v.agent.as_ref().is_some_and(|a| a.session_id.is_some());
        let resume = if resume_on && !already_open { launch::resume_for(path, known) } else { Default::default() };
        let result = match launch::resolve_open_action(resume_on, already_open, ide, resume) {
            OpenAction::Plain => launch::open_in_editor(path, ide),
            OpenAction::TerminalClaude(r) => launch::open_terminal(path, Some(&launch::claude_command(&r))),
            OpenAction::CopyAndOpen(r) => {
                let cmd = launch::claude_command(&r);
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

pub fn open_claude(v: WorktreeView, tx: Sender<Msg>) {
    std::thread::spawn(move || {
        let path = Path::new(&v.worktree.path);
        let known = v.agent.as_ref().is_some_and(|a| a.session_id.is_some());
        let cmd = launch::claude_command(&launch::resume_for(path, known));
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
