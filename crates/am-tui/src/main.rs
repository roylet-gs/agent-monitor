//! `am-tui` — native dashboard (Phase 1: read-only).
//!
//! Runs on the alternate screen and draws only when data, input or the
//! clock-second changes. Every frame is wrapped in synchronized-output
//! escapes (DEC mode 2026) so terminals that support it present it atomically.

mod app;
mod data;
mod jobs;
mod launcher;
mod modals;
mod ops;
mod plugins_ui;
mod settings_ui;
mod setup;
mod theme;
mod theme_builder;
mod ui;

use std::io::{self, Write};
use std::time::{Duration, Instant};

use am_core::{info, settings};
use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use ratatui::crossterm::terminal::{BeginSynchronizedUpdate, EndSynchronizedUpdate};
use ratatui::crossterm::execute;

use app::{Action, App, Key, Mouse};

fn map_key(code: KeyCode, mods: KeyModifiers) -> Option<Key> {
    let ctrl = mods.contains(KeyModifiers::CONTROL);
    Some(match code {
        KeyCode::Char('c') if ctrl => Key::CtrlC,
        KeyCode::Char('d') if ctrl => Key::CtrlD,
        KeyCode::Char('u') if ctrl => Key::CtrlU,
        KeyCode::Char('s') if ctrl => Key::CtrlS,
        KeyCode::Char(c) => Key::Char(c),
        KeyCode::Up => Key::Up,
        KeyCode::Left => Key::Left,
        KeyCode::Right => Key::Right,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Down => Key::Down,
        KeyCode::PageUp => Key::PageUp,
        KeyCode::PageDown => Key::PageDown,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        KeyCode::Tab => Key::Tab,
        KeyCode::BackTab => Key::BackTab,
        KeyCode::Enter => Key::Enter,
        KeyCode::Esc => Key::Esc,
        _ => return None,
    })
}

fn main() -> io::Result<()> {
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("am-tui {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let settings = settings::load();
    am_core::log::init(settings.log_level(), settings.max_log_size_mb);
    info!("tui", "am-tui {} starting (theme={})", env!("CARGO_PKG_VERSION"), settings.theme);


    let collector = data::Collector::spawn(settings.clone());
    let args: Vec<String> = std::env::args().skip(1).collect();
    // `am setup` → `--setup` reopens the wizard; `am --watch` → `--log` opens the log pane.
    let first_run = setup::needed(&settings) || args.iter().any(|a| a == "--setup");
    let show_log = args.iter().any(|a| a == "--log");
    let setup = first_run.then(|| setup::Setup::new(Vec::new(), &settings));
    let mut app = App::new(settings);
    app.setup = setup;
    app.show_log = show_log;
    // Hooks pointing at a deleted am-hook (removed worktree, upgraded install)
    // fail on every agent event; re-point them at the current binary.
    let repaired = am_core::hooks_installer::repair_dangling_hooks();
    if !repaired.is_empty() {
        let names: Vec<_> = repaired.iter().map(|k| k.provider().name()).collect();
        app.notice = Some(format!("Repaired {} hooks (am-hook had moved)", names.join(" + ")));
    }

    // ratatui::init enables raw mode + alt screen and installs a panic hook
    // that restores the terminal before printing the panic.
    let mut terminal = ratatui::init();
    // ratatui's hook doesn't know about mouse capture; turn it off first so
    // a panic doesn't leave the shell receiving mouse escape sequences.
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = execute!(io::stdout(), DisableMouseCapture);
        hook(info);
    }));
    set_mouse(app.settings.mouse_enabled);
    let result = run(&mut terminal, &mut app, &collector);
    set_mouse(false);
    ratatui::restore();
    collector.shutdown();
    result
}

/// Turn terminal mouse reporting on or off.
fn set_mouse(on: bool) {
    let r = if on { execute!(io::stdout(), EnableMouseCapture) } else { execute!(io::stdout(), DisableMouseCapture) };
    match r {
        Ok(()) => am_core::debug!("tui", "mouse capture {}", if on { "on" } else { "off" }),
        Err(e) => am_core::warn!("tui", "Couldn't turn mouse capture {}: {e}", if on { "on" } else { "off" }),
    }
}

/// Two left clicks on the same row within this window are a double-click.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

fn run(terminal: &mut ratatui::DefaultTerminal, app: &mut App, collector: &data::Collector) -> io::Result<()> {
    let mut theme = theme::Theme::load(&app.settings.theme);
    let (launch_tx, launch_rx) = std::sync::mpsc::channel();
    launcher::spawn_update_check(launch_tx.clone());
    let mut popup_since: Option<Instant> = None;
    let mut log_lines: Vec<String> = Vec::new();
    let mut dirty = true;
    let mut last_second = am_core::timefmt::now_unix();
    let mut notice_since: Option<Instant> = None;
    let mut last_click: Option<(u16, Instant)> = None;
    loop {
        while let Some(snapshot) = collector.try_recv() {
            app.apply(snapshot);
            for sound in app.sounds.drain(..) {
                play_sound(&sound);
            }
            dirty = true;
        }
        while let Ok(msg) = launch_rx.try_recv() {
            match msg {
                launcher::Msg::Popup(p) => {
                    if matches!(app.modal, Some(modals::Modal::Busy { .. })) {
                        app.modal = None;
                    }
                    app.popup = Some(p);
                    popup_since = Some(Instant::now());
                }
                launcher::Msg::Notice(n) => {
                    app.notice = Some(n);
                    notice_since = None;
                }
                launcher::Msg::BranchChecked(branch, r) => {
                    if let Some(m) = &mut app.modal {
                        m.on_check_result(branch, r);
                    }
                }
                launcher::Msg::JobStep(job, text) => {
                    app.jobs.step(job, text);
                    app.rebuild_rows();
                }
                launcher::Msg::JobLog(job, line) => app.jobs.log(job, line),
                launcher::Msg::Created(job, result) => {
                    use am_core::worktrees::CreateError;
                    match result {
                        Ok(w) => {
                            app.notice = Some(format!("Created {}", w.branch));
                            app.pending_select = Some(w.id.clone());
                            app.jobs.set_worktree(job, w.id.clone());
                            let repo_id = w.repo_id.clone();
                            if am_core::worktrees::has_script(&repo_id) {
                                app.jobs.ask(job, jobs::Question::RunScript { repo_id, worktree_path: w.path.clone() });
                            } else {
                                app.jobs.finish(job, Ok(()), Instant::now());
                            }
                            collector.refresh(false);
                        }
                        Err(CreateError::BranchExists { local, remote, .. }) if local || remote => {
                            app.jobs.ask(job, jobs::Question::BranchExists { local, remote });
                        }
                        Err(e) => app.jobs.finish(job, Err(e.to_string()), Instant::now()),
                    }
                    app.rebuild_rows();
                }
                launcher::Msg::ScriptDone(job, result) => app.jobs.finish(job, result, Instant::now()),
                launcher::Msg::RepoAdded(path, result) if app.setup.is_some() => {
                    app.modal = None;
                    if result.is_ok() {
                        collector.refresh(false);
                    }
                    if let Some(s) = &mut app.setup {
                        s.on_repo_added(path, result);
                    }
                }
                launcher::Msg::RepoAdded(path, result) => match result {
                    Ok(name) => {
                        app.modal = None;
                        app.notice = Some(format!("Added repository {name}"));
                        collector.refresh(false);
                    }
                    Err(e) => app.modal = Some(modals::Modal::AddRepo { path, error: Some(e) }),
                },
                launcher::Msg::UpdateAvailable(update) => app.update_available = Some(update),
                launcher::Msg::Deleted(job, id, label, result) => {
                    app.finish_delete(&id, result.is_ok());
                    match result {
                        Ok(warnings) => {
                            if warnings.is_empty() {
                                app.notice = Some(format!("Deleted {label}"));
                            } else {
                                app.jobs.log(job, "Deleted, with warnings:".into());
                            }
                            for w in warnings {
                                app.jobs.log(job, w);
                            }
                            app.jobs.finish(job, Ok(()), Instant::now());
                        }
                        Err(e) => app.jobs.finish(job, Err(e), Instant::now()),
                    }
                    collector.refresh(false);
                }
            }
            dirty = true;
        }
        if let Some((repo_path, branch)) = app.modal.as_mut().and_then(|m| m.due_check()) {
            ops::check_branch(repo_path, branch, launch_tx.clone());
        }
        if app.popup.is_some() && popup_since.is_some_and(|t| t.elapsed() > Duration::from_secs(6)) {
            app.popup = None;
            popup_since = None;
            dirty = true;
        }
        let now = am_core::timefmt::now_unix();
        if now != last_second {
            // Elapsed/age labels tick once a second; the diff renderer only
            // rewrites the cells that actually changed.
            last_second = now;
            if app.jobs.tick(Instant::now()) {
                app.rebuild_rows();
            }
            if app.show_log {
                log_lines = tail_log(200);
            }
            dirty = true;
        }
        if app.notice.is_some() && notice_since.get_or_insert_with(Instant::now).elapsed() > Duration::from_secs(2) {
            app.notice = None;
            notice_since = None;
            dirty = true;
        }
        if dirty {
            execute!(io::stdout(), BeginSynchronizedUpdate)?;
            terminal.draw(|f| ui::draw(f, app, &theme, now, &log_lines))?;
            execute!(io::stdout(), EndSynchronizedUpdate)?;
            io::stdout().flush()?;
            dirty = false;
        }

        if event::poll(Duration::from_millis(100))? {
            let action = match event::read()? {
                Event::Key(k) if k.kind != KeyEventKind::Release => map_key(k.code, k.modifiers).map(|key| app.on_key(key)),
                Event::Mouse(m) if app.settings.mouse_enabled => {
                    let mouse = match m.kind {
                        MouseEventKind::ScrollDown => Some(Mouse::Scroll(1)),
                        MouseEventKind::ScrollUp => Some(Mouse::Scroll(-1)),
                        MouseEventKind::Down(MouseButton::Left) => {
                            let double = last_click.is_some_and(|(row, t)| row == m.row && t.elapsed() < DOUBLE_CLICK);
                            // A third click starts over rather than making a second double.
                            last_click = if double { None } else { Some((m.row, Instant::now())) };
                            Some(Mouse::Click { x: m.column, y: m.row, double })
                        }
                        // Moves, drags, releases and other buttons don't redraw.
                        _ => None,
                    };
                    match mouse {
                        Some(mouse) => {
                            let size = terminal.size()?;
                            Some(app.on_mouse(mouse, ratatui::layout::Rect::new(0, 0, size.width, size.height)))
                        }
                        None => None,
                    }
                }
                Event::Resize(..) => {
                    dirty = true;
                    None
                }
                _ => None,
            };
            if let Some(action) = action {
                if dispatch(action, terminal, app, collector, &mut theme, &launch_tx)? {
                    return Ok(());
                }
                for sound in app.sounds.drain(..) {
                    play_sound(&sound);
                }
                if app.show_log && log_lines.is_empty() {
                    log_lines = tail_log(200);
                }
                dirty = true;
            }
        }
    }
}

/// Run an `Action` from a key or mouse press. Returns `true` to quit.
fn dispatch(
    action: Action,
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut App,
    collector: &data::Collector,
    theme: &mut theme::Theme,
    launch_tx: &std::sync::mpsc::Sender<launcher::Msg>,
) -> io::Result<bool> {
    let mouse_before = app.settings.mouse_enabled;
    match action {
        Action::Quit => return Ok(true),
        Action::Refresh => collector.refresh(true),
        Action::Open(v) => launcher::open(*v, &app.settings, launch_tx.clone()),
        Action::OpenTerminal(v) => launcher::open_terminal(*v, launch_tx.clone()),
        Action::OpenAgent(v) => launcher::open_agent(*v, &app.settings, launch_tx.clone()),
        Action::OpenUrl(url) => launcher::open_url(url, launch_tx.clone()),
        Action::SettingsChanged => {
            *theme = theme::Theme::load(&app.settings.theme);
            collector.update_settings(app.settings.clone());
        }
        Action::Create(job, repo, req) => ops::create(app.settings.clone(), *repo, *req, job, launch_tx.clone()),
        Action::Delete(job, v, delete_branch) => ops::delete(*v, delete_branch, job, launch_tx.clone()),
        Action::AddRepo(path) => ops::add_repo(path, launch_tx.clone()),
        Action::Suggest { title, body } => ops::suggest(title, body, launch_tx.clone()),
        Action::Train(mode) => launcher::train(mode, launch_tx.clone()),
        Action::InstallHooks => {
            // Every agent the wizard listed; the first failure is reported.
            let kinds: Vec<_> = app.setup.as_ref().map(|s| s.agents.iter().map(|(k, _)| *k).collect()).unwrap_or_default();
            let mut r = Ok(None);
            for kind in kinds {
                match am_core::hooks_installer::install_hooks(kind) {
                    Ok(bin) => r = r.map(|prev: Option<String>| prev.or(bin.map(|b| b.display().to_string()))),
                    Err(e) => {
                        r = Err(format!("{}: {e}", kind.provider().name()));
                        break;
                    }
                }
            }
            if let Some(s) = &mut app.setup {
                s.on_hooks_installed(r);
            }
        }
        Action::RemoveStandalone(path) => {
            if let Ok(conn) = am_core::db::open() {
                let _ = am_core::db::remove_standalone_session(&conn, &path);
            }
            app.notice = Some("Stopped tracking session".into());
            collector.refresh(false);
        }
        Action::RunScript { job, repo_id, worktree_path } => {
            ops::run_script(repo_id, worktree_path, app.settings.startup_script_in_window(), job, launch_tx.clone());
        }
        Action::EditSettingsFile => {
            edit_settings_file(terminal, mouse_before)?;
            app.set_settings(settings::load());
            *theme = theme::Theme::load(&app.settings.theme);
            collector.update_settings(app.settings.clone());
        }
        Action::None => {}
    }
    if app.settings.mouse_enabled != mouse_before {
        info!("tui", "Mouse {}", if app.settings.mouse_enabled { "enabled" } else { "disabled" });
        set_mouse(app.settings.mouse_enabled);
    }
    Ok(false)
}

/// macOS system sound by name; fire-and-forget (port of src/lib/audio.ts).
fn play_sound(name: &str) {
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric()) {
        am_core::warn!("audio", "Ignoring invalid sound name {name:?}");
        return;
    }
    let path = format!("/System/Library/Sounds/{name}.aiff");
    match std::process::Command::new("afplay")
        .arg(&path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(child) => {
            info!("audio", "Playing {name}");
            // Reap it (and surface afplay errors) without blocking the UI.
            std::thread::spawn(move || {
                if let Ok(out) = child.wait_with_output()
                    && !out.status.success()
                {
                    am_core::warn!("audio", "afplay {path} failed: {}", String::from_utf8_lossy(&out.stderr).trim());
                }
            });
        }
        Err(e) => am_core::warn!("audio", "Couldn't run afplay for {name}: {e}"),
    }
}

/// Last `n` lines of debug.log (reads at most the final 64 KiB).
fn tail_log(n: usize) -> Vec<String> {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(am_core::paths::log_path()) else { return Vec::new() };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let _ = f.seek(SeekFrom::Start(len.saturating_sub(64 * 1024)));
    // Seeking can land mid-character; decode lossily rather than failing.
    let mut bytes = Vec::new();
    let _ = f.read_to_end(&mut bytes);
    let buf = String::from_utf8_lossy(&bytes);
    let lines: Vec<String> = buf.lines().map(str::to_owned).collect();
    lines[lines.len().saturating_sub(n)..].to_vec()
}

/// Hand the terminal to $EDITOR (default `vi`) for settings.json, then take it back.
fn edit_settings_file(terminal: &mut ratatui::DefaultTerminal, mouse: bool) -> io::Result<()> {
    let path = am_core::paths::settings_path();
    let editor = std::env::var("VISUAL").or_else(|_| std::env::var("EDITOR")).unwrap_or_else(|_| "vi".into());
    set_mouse(false);
    ratatui::restore();
    // $EDITOR may carry arguments (e.g. `code --wait`), so run it via the shell.
    let status = std::process::Command::new("sh").arg("-c").arg(format!("{editor} \"$1\"")).arg("sh").arg(&path).status();
    *terminal = ratatui::init();
    set_mouse(mouse);
    terminal.clear()?;
    if let Err(e) = status {
        am_core::warn!("settings", "Failed to run editor {editor}: {e}");
    }
    Ok(())
}
