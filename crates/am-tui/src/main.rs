//! `am-tui` — native dashboard (Phase 1: read-only).
//!
//! Runs on the alternate screen and draws only when data, input or the
//! clock-second changes. Every frame is wrapped in synchronized-output
//! escapes (DEC mode 2026) so terminals that support it present it atomically.

mod app;
mod data;
mod folder_browser;
mod jobs;
mod keymap;
mod keys_ui;
mod launcher;
mod modals;
mod ops;
mod plugins_ui;
mod repos_ui;
mod reviews_ui;
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
    let setup = first_run.then(|| setup::Setup::new(repo_names(), &settings));
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

/// Names of the registered repositories, listed as already added by the
/// setup wizard's Repositories step.
fn repo_names() -> Vec<String> {
    match am_core::db::open().and_then(|conn| am_core::db::repositories(&conn)) {
        Ok(repos) => repos.into_iter().map(|r| r.name).collect(),
        Err(e) => {
            am_core::warn!("tui", "Couldn't list repositories for setup: {e}");
            Vec::new()
        }
    }
}

/// Close the settings editor and open the setup wizard over the current settings.
fn open_setup(app: &mut App) {
    app.settings_editor = None;
    app.setup = Some(setup::Setup::new(repo_names(), &app.settings));
    info!("tui", "Setup wizard opened from settings");
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
    let mut settings_mtime = settings_file_mtime();
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
                    use am_core::worktrees::{CreateError, CreateMode};
                    match result {
                        Ok(w) => {
                            app.notice = Some(format!("Created {}", w.branch));
                            app.pending_select = Some(w.id.clone());
                            app.jobs.set_worktree(job, w.id.clone());
                            let repo_id = w.repo_id.clone();
                            // A tmp-space worktree is only there to look at:
                            // no startup script.
                            if !w.tmp_space && am_core::worktrees::has_script(&repo_id) {
                                app.jobs.ask(job, jobs::Question::RunScript { repo_id, worktree_path: w.path.clone() });
                            } else {
                                app.jobs.finish(job, Ok(()), Instant::now());
                            }
                            collector.refresh(false);
                        }
                        // Same choices as NewForm::request, for an Enter
                        // pressed before the live check answered: pull a
                        // branch that is only on origin; in the tmp space
                        // reuse one that is only local.
                        Err(CreateError::BranchExists { local, remote, path_exists: false })
                            if remote != local
                                && (remote
                                    || app.jobs.get(job).is_some_and(|j| j.pending_create().is_some_and(|(_, req, _)| req.tmp_space))) =>
                        {
                            let mode = if remote { CreateMode::TrackRemote } else { CreateMode::ReuseLocal };
                            if let jobs::JobEffect::Create(job, repo, req) = app.jobs.rerun_create(job, mode) {
                                info!("tui", "Create of {}: branch exists (local={local}, remote={remote}), using {mode:?}", req.branch);
                                ops::create(app.settings.clone(), *repo, *req, job, launch_tx.clone());
                            }
                        }
                        Err(CreateError::BranchExists { local, remote, .. }) if local || remote => {
                            app.jobs.ask(job, jobs::Question::BranchExists { local, remote });
                        }
                        Err(e) => {
                            // The review page hides the jobs pane: say why here.
                            if app.reviews.is_some() {
                                app.notice = Some(format!("Pull failed: {e}"));
                            }
                            app.jobs.finish(job, Err(e.to_string()), Instant::now());
                        }
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
                        if let Some(e) = &mut app.settings_editor {
                            e.refresh_repos(Some((true, format!("Added {name} ({path})"))));
                            if let Some(page) = &mut e.repos {
                                page.select_name(&name);
                            }
                        }
                        collector.refresh(false);
                    }
                    Err(e) => match &mut app.modal {
                        Some(modals::Modal::AddRepo(browser)) => browser.add_failed(e),
                        _ => app.popup = Some(format!("✗ Couldn't add {path}: {e}")),
                    },
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
                        Err(e) => {
                            if app.reviews.is_some() {
                                app.notice = Some(format!("Couldn't delete {label}: {e}"));
                            }
                            app.jobs.finish(job, Err(e), Instant::now());
                        }
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
            // settings.json edited outside the TUI (e.g. `e` opened it in the IDE).
            let mtime = settings_file_mtime();
            if mtime != settings_mtime {
                settings_mtime = mtime;
                let loaded = settings::load();
                if loaded != app.settings {
                    info!("settings", "settings.json changed on disk, reloading");
                    let mouse_before = app.settings.mouse_enabled;
                    app.set_settings(loaded);
                    theme = theme::Theme::load(&app.settings.theme);
                    collector.update_settings(app.settings.clone());
                    if app.settings.mouse_enabled != mouse_before {
                        set_mouse(app.settings.mouse_enabled);
                    }
                }
            }
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
                        MouseEventKind::Moved => Some(Mouse::Move { x: m.column, y: m.row }),
                        // Drags, releases and other buttons don't redraw.
                        _ => None,
                    };
                    match mouse {
                        // A move only redraws when it lands on something else to highlight.
                        Some(mouse @ Mouse::Move { x, y }) => {
                            let before = app.hover.and_then(|(x, y)| ui::hit_at(&app.hits.borrow(), x, y));
                            app.on_mouse(mouse);
                            dirty |= ui::hit_at(&app.hits.borrow(), x, y) != before;
                            None
                        }
                        Some(mouse) => Some(app.on_mouse(mouse)),
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
        Action::RefreshReviews => collector.refresh_reviews(),
        Action::Open(v) => launcher::open(*v, &app.settings, launch_tx.clone()),
        Action::OpenUrl(url) => launcher::open_url(url, launch_tx.clone()),
        Action::SettingsChanged => {
            *theme = theme::Theme::load(&app.settings.theme);
            collector.update_settings(app.settings.clone());
        }
        Action::Create(job, repo, req) => ops::create(app.settings.clone(), *repo, *req, job, launch_tx.clone()),
        Action::Delete(job, v, delete_branch) => ops::delete(*v, delete_branch, job, launch_tx.clone()),
        Action::AddRepo(path) => ops::add_repo(path, launch_tx.clone()),
        Action::ReposChanged => collector.refresh(false),
        Action::EditScript(repo_id) => {
            let path = match am_core::worktrees::create_script(&repo_id) {
                Ok(p) => p,
                Err(e) => {
                    am_core::warn!("scripts", "Couldn't create the startup script for {repo_id}: {e}");
                    app.popup = Some(format!("✗ Couldn't create the startup script: {e}"));
                    return Ok(false);
                }
            };
            am_core::info!("scripts", "Editing startup script {}", path.display());
            let shown = am_core::hooks_installer::tilde(&path);
            let ide = am_core::launch::IdeChoice::from_setting(&app.settings.ide);
            if ide != am_core::launch::IdeChoice::Terminal {
                launcher::open_file(path, ide, app.settings.ide_command.clone(), launch_tx.clone());
            } else {
                edit_file(terminal, mouse_before, &path)?;
            }
            if let Some(e) = &mut app.settings_editor {
                e.refresh_repos(Some((true, format!("Startup script: {shown}"))));
            }
        }
        Action::Suggest { title, body } => ops::suggest(title, body, launch_tx.clone()),
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
            let ide = am_core::launch::IdeChoice::from_setting(&app.settings.ide);
            if ide != am_core::launch::IdeChoice::Terminal {
                launcher::open_file(am_core::paths::settings_path(), ide, app.settings.ide_command.clone(), launch_tx.clone());
                return Ok(false);
            }
            edit_file(terminal, mouse_before, &am_core::paths::settings_path())?;
            app.set_settings(settings::load());
            *theme = theme::Theme::load(&app.settings.theme);
            collector.update_settings(app.settings.clone());
        }
        Action::ViewLogs => {
            use am_core::launch::{IdeChoice, LogView};
            let files = am_core::paths::log_files();
            let view = am_core::launch::log_view(&files);
            info!("tui", "View logs: {view:?} ({} file(s))", files.len());
            match view {
                LogView::Missing => app.popup = Some(format!("No log file yet at {}", am_core::paths::log_path().display())),
                LogView::Reveal(path) => launcher::reveal(path, launch_tx.clone()),
                LogView::Edit(path) => {
                    let ide = IdeChoice::from_setting(&app.settings.ide);
                    if ide == IdeChoice::Terminal {
                        edit_file(terminal, mouse_before, &path)?;
                    } else {
                        launcher::open_file(path, ide, app.settings.ide_command.clone(), launch_tx.clone());
                    }
                }
            }
        }
        Action::RunSetup => open_setup(app),
        Action::ResetSettings => {
            // Dropping setupCompleted means quitting mid-wizard shows it again next start.
            let next = settings::reset_to_defaults(settings::load(), false);
            match settings::save(&next) {
                Ok(()) => {
                    info!("settings", "Settings reset to defaults from the TUI");
                    // Re-read so `raw` matches the file (the mtime watch compares against it).
                    app.set_settings(settings::load());
                    *theme = theme::Theme::load(&app.settings.theme);
                    collector.update_settings(app.settings.clone());
                    open_setup(app);
                }
                Err(e) => {
                    am_core::warn!("settings", "Couldn't reset settings: {e}");
                    app.notice = Some(format!("Couldn't reset settings: {e}"));
                }
            }
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

fn settings_file_mtime() -> Option<std::time::SystemTime> {
    std::fs::metadata(am_core::paths::settings_path()).and_then(|m| m.modified()).ok()
}

/// Hand the terminal to $EDITOR (default `vi`) for a file (settings.json, a
/// startup script, debug.log), then take it back.
fn edit_file(terminal: &mut ratatui::DefaultTerminal, mouse: bool, path: &std::path::Path) -> io::Result<()> {
    let editor = std::env::var("VISUAL").or_else(|_| std::env::var("EDITOR")).unwrap_or_else(|_| "vi".into());
    set_mouse(false);
    ratatui::restore();
    // $EDITOR may carry arguments (e.g. `code --wait`), so run it via the shell.
    let status = std::process::Command::new("sh").arg("-c").arg(format!("{editor} \"$1\"")).arg("sh").arg(path).status();
    *terminal = ratatui::init();
    set_mouse(mouse);
    terminal.clear()?;
    if let Err(e) = status {
        am_core::warn!("tui", "Failed to run editor {editor} on {}: {e}", path.display());
    }
    Ok(())
}
