//! `am-tui` — native dashboard (Phase 1: read-only).
//!
//! Runs on the alternate screen and draws only when data, input or the
//! clock-second changes. Every frame is wrapped in synchronized-output
//! escapes (DEC mode 2026) so terminals that support it present it atomically.

am_core::silence_unit_tests!();

mod app;
mod cmux_sync;
mod data;
mod folder_browser;
mod gitwork;
mod jobs;
mod keymap;
mod keys_ui;
mod launcher;
mod modals;
mod nav;
mod ops;
mod plugins_ui;
mod repos_ui;
mod review_groups_ui;
mod reviews_ui;
mod settings_ui;
mod setup;
mod theme;
mod theme_builder;
mod ui;
mod usage_reports;

use std::io::{self, Write};
use std::time::{Duration, Instant};

use am_core::worktrees::ScriptKind;
use am_core::{info, settings};
use std::sync::atomic::{AtomicBool, Ordering};

use ratatui::crossterm::event::{
    self, DisableFocusChange, DisableMouseCapture, EnableFocusChange, EnableMouseCapture, Event, KeyCode, KeyEvent,
    KeyEventKind, KeyModifiers, KeyboardEnhancementFlags, ModifierKeyCode, MouseButton, MouseEventKind,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use ratatui::crossterm::terminal::{BeginSynchronizedUpdate, EndSynchronizedUpdate};
use ratatui::crossterm::execute;

use app::{Action, App, Held, Key, Mouse};

/// Whether the terminal speaks the kitty keyboard protocol (kitty, Ghostty,
/// WezTerm, recent iTerm2…), checked once at startup. Only then are
/// modifier keys reported on their own, so the key bar can follow them.
static KEYBOARD_ENHANCED: AtomicBool = AtomicBool::new(false);

/// Turn the kitty keyboard protocol on or off (no-op where unsupported).
/// Alternate keys keep Shift+v arriving as `V` and Shift+/ as `?`.
fn set_keyboard(on: bool) {
    if !KEYBOARD_ENHANCED.load(Ordering::Relaxed) {
        return;
    }
    let flags = KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
        | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
        | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS
        | KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES;
    let r = if on {
        execute!(io::stdout(), PushKeyboardEnhancementFlags(flags), EnableFocusChange)
    } else {
        execute!(io::stdout(), PopKeyboardEnhancementFlags, DisableFocusChange)
    };
    if let Err(e) = r {
        am_core::warn!("tui", "Couldn't {} the kitty keyboard protocol: {e}", if on { "enable" } else { "disable" });
    }
}

/// The held modifiers after a key event: a Shift/Ctrl key pressed or
/// released on its own, or Ctrl reported on another key. (Shift can't be
/// read off other keys: an alternate key such as `V` arrives without it.)
fn held_after(held: Held, k: &KeyEvent) -> Held {
    let down = k.kind != KeyEventKind::Release;
    match k.code {
        KeyCode::Modifier(ModifierKeyCode::LeftShift | ModifierKeyCode::RightShift) => Held { shift: down, ..held },
        KeyCode::Modifier(ModifierKeyCode::LeftControl | ModifierKeyCode::RightControl) => Held { ctrl: down, ..held },
        KeyCode::Modifier(_) => held,
        _ => Held { ctrl: k.modifiers.contains(KeyModifiers::CONTROL), ..held },
    }
}

fn map_key(code: KeyCode, mods: KeyModifiers) -> Option<Key> {
    let ctrl = mods.contains(KeyModifiers::CONTROL);
    Some(match code {
        KeyCode::Char('c') if ctrl => Key::CtrlC,
        KeyCode::Char('d') if ctrl => Key::CtrlD,
        KeyCode::Char('u') if ctrl => Key::CtrlU,
        KeyCode::Char('s') if ctrl => Key::CtrlS,
        KeyCode::Char('v') if ctrl => Key::CtrlV,
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
        KeyCode::Enter if ctrl => Key::CtrlEnter,
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
    let _telemetry = am_core::telemetry::init(am_core::telemetry::Binary::Tui, &settings);
    info!("tui", "am-tui {} starting (theme={})", env!("CARGO_PKG_VERSION"), settings.theme);


    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--reviews") {
        // A review window (`am reviews`, `ctrl-v`, the Dock's Reviews) sits
        // beside a dashboard, which owns the cmux pills and notifications.
        cmux_sync::role::give_up();
    }
    let collector = data::Collector::spawn(settings.clone());
    // `am setup` → `--setup` reopens the wizard; `am --watch` → `--log` opens the log pane.
    let first_run = setup::needed(&settings) || args.iter().any(|a| a == "--setup");
    let show_log = args.iter().any(|a| a == "--log");
    // `ctrl-v` / `am reviews` open `am-tui --reviews`: just the review page.
    let reviews_window = !first_run && args.iter().any(|a| a == "--reviews");
    let reviews_mode = !first_run && settings.reviews_mode();
    let setup = first_run.then(|| setup::Setup::new(repo_names(), &settings));
    let mut app = App::new(settings);
    app.setup = setup;
    app.show_log = show_log;
    if reviews_mode {
        // Review-only mode: the page even with review requests off (it says
        // how to turn them on).
        app.enter_reviews_mode();
    } else if reviews_window {
        // Review requests off: say why, on the full dashboard.
        if app.open_reviews(false) == app::Action::RefreshReviews {
            app.reviews_window = true;
            app.review_fetch_due = true;
        }
    }
    if std::mem::take(&mut app.review_fetch_due) {
        collector.refresh_reviews();
    }
    // Set up before usage reports existed: ask once (not in a review window,
    // which sits beside a dashboard that asks; not when the environment opts out).
    if !first_run
        && !reviews_window
        && am_core::telemetry::consent(&app.settings, |n| std::env::var(n).ok()) == am_core::telemetry::Consent::Ask
    {
        app.modal = Some(modals::Modal::Telemetry);
    }
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
        set_keyboard(false);
        hook(info);
    }));
    set_mouse(app.settings.mouse_enabled);
    let enhanced = matches!(ratatui::crossterm::terminal::supports_keyboard_enhancement(), Ok(true));
    info!("tui", "Kitty keyboard protocol {}", if enhanced { "supported: modifier keys update the key bar" } else { "not supported" });
    KEYBOARD_ENHANCED.store(enhanced, Ordering::Relaxed);
    app.modifiers_reported = enhanced;
    set_keyboard(true);
    let result = run(&mut terminal, &mut app, &collector);
    record_features(&mut app);
    set_keyboard(false);
    set_mouse(false);
    ratatui::restore();
    collector.shutdown();
    result
}

/// The day's `am.heartbeat`, once the first snapshot of the day is in.
fn record_heartbeat(app: &App) {
    use am_core::telemetry::{public, upload};
    if !public::is_on() {
        return;
    }
    // Snapshots come often: the day is checked in memory, the file once a day.
    static DONE_DAY: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());
    let today = usage_reports::today();
    let Ok(mut done) = DONE_DAY.lock() else { return };
    if *done == today {
        return;
    }
    *done = today.clone();
    let mut state = upload::state();
    if state.last_heartbeat_day.as_deref() == Some(today.as_str()) {
        return;
    }
    let terminal = std::env::var("TERM_PROGRAM").ok();
    let event = usage_reports::heartbeat(&app.settings, &app.snapshot, am_core::cmux::inside(), terminal.as_deref(), public::now_ms());
    am_core::telemetry::record(event);
    state.last_heartbeat_day = Some(today);
    upload::save_state(&state);
}

/// `am.features` for the commands run since the last one.
fn record_features(app: &mut App) {
    for e in usage_reports::features(&mut app.command_counts, am_core::telemetry::public::now_ms()) {
        am_core::telemetry::record(e);
    }
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

/// Input events handled between two draws at most.
const MAX_EVENTS_PER_FRAME: usize = 64;

fn run(terminal: &mut ratatui::DefaultTerminal, app: &mut App, collector: &data::Collector) -> io::Result<()> {
    let mut theme = theme::Theme::load(&app.settings.theme);
    let (launch_tx, launch_rx) = std::sync::mpsc::channel();
    launcher::spawn_update_check(launch_tx.clone());
    let mut popup_since: Option<Instant> = None;
    let mut log_lines: Vec<String> = Vec::new();
    let mut log_len: Option<u64> = None;
    let mut dirty = true;
    let mut last_second = am_core::timefmt::now_unix();
    // Stage timings (`am perf`); `input_at` is when the oldest undrawn input arrived.
    let mut perf = am_core::perf::Perf::new();
    let mut input_at: Option<Instant> = None;
    // When the notice on screen appeared, and its text: a new notice (from a
    // key or the launcher) gets its own 2 s, not what's left of the last one's.
    let mut notice_since: Option<(Instant, String)> = None;
    let mut last_click: Option<(u16, Instant)> = None;
    let mut settings_mtime = settings_file_mtime();
    // Usage reports: what `telemetry` was last applied, and when features were last recorded.
    let mut telemetry_applied = app.settings.telemetry.clone();
    let mut features_at = Instant::now();
    usage_reports::spawn_uploader();
    loop {
        while let Some(snapshot) = collector.try_recv() {
            app.apply(snapshot);
            record_heartbeat(app);
            for sound in app.sounds.drain(..) {
                play_sound(&sound);
            }
            let alerts = std::mem::take(&mut app.review_alerts);
            if !alerts.is_empty() && am_core::cmux::inside() && cmux_sync::role::owned() {
                std::thread::spawn(move || {
                    for (title, body) in alerts {
                        let _ = am_core::cmux::notify(&am_core::cmux::SystemCmux, &title, &body, None);
                    }
                });
            }
            dirty = true;
        }
        while let Ok(msg) = launch_rx.try_recv() {
            match msg {
                launcher::Msg::Popup(p) => {
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
                            let review = app.jobs.get(job).is_some_and(|j| j.review_script().is_some());
                            // "Pull + run script": the review script runs at
                            // once, here in the pane (one just made from the
                            // template waits until it's been saved).
                            // Otherwise a tmp-space worktree is only there to
                            // look at: no startup script.
                            let unsaved = app.jobs.get(job).and_then(|j| j.await_save.clone()).is_some_and(|(p, at)| modified(&p) == Some(at));
                            if review && unsaved {
                                info!("tui", "Pulled {}; waiting for its review script to be saved", w.branch);
                                app.jobs.ask(job, jobs::Question::SaveReviewScript { repo_id, worktree_path: w.path.clone() });
                            } else if review {
                                run_review_script(app, job, repo_id, w.path.clone(), &launch_tx);
                            } else if !w.tmp_space && am_core::worktrees::has_script(&repo_id) {
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
                launcher::Msg::ScriptDone(job, result) => {
                    // The review page hides the jobs pane: say why here.
                    if let Err(e) = &result
                        && app.reviews.is_some()
                        && app.jobs.get(job).is_some_and(|j| j.review_script().is_some())
                    {
                        app.notice = Some(format!("Review script failed: {e}"));
                    }
                    app.jobs.finish(job, result, Instant::now());
                }
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
            // Usage reports follow the setting however it changed (Settings, the
            // wizard, the prompt, an edited file).
            if app.settings.telemetry != telemetry_applied {
                telemetry_applied = app.settings.telemetry.clone();
                am_core::telemetry::public::apply(&app.settings);
            }
            if features_at.elapsed() >= usage_reports::EVERY {
                features_at = Instant::now();
                record_features(app);
            }
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
                    if std::mem::take(&mut app.review_fetch_due) {
                        collector.refresh_reviews();
                    }
                    if app.settings.mouse_enabled != mouse_before {
                        set_mouse(app.settings.mouse_enabled);
                    }
                }
            }
            if app.jobs.tick(Instant::now()) {
                app.rebuild_rows();
            }
            // Review scripts made from the template run once they're saved.
            for (job, repo_id, worktree_path, path, at) in app.jobs.awaiting_save() {
                match modified(&path) {
                    Some(m) if m == at => {}
                    Some(_) => {
                        info!("tui", "Review script {} saved; running it in {worktree_path}", path.display());
                        run_review_script(app, job, repo_id, worktree_path, &launch_tx);
                    }
                    None => {
                        app.jobs.finish(job, Err(format!("{} was removed", path.display())), Instant::now());
                    }
                }
            }
            if app.show_log {
                // Only re-read once something was appended (or it rotated).
                let len = std::fs::metadata(am_core::paths::log_path()).map(|m| m.len()).ok();
                if len != log_len || log_lines.is_empty() {
                    log_len = len;
                    log_lines = tail_log(200);
                }
            }
            dirty = true;
        }
        match &app.notice {
            Some(n) if notice_since.as_ref().is_none_or(|(_, shown)| shown != n) => {
                notice_since = Some((Instant::now(), n.clone()));
            }
            Some(_) if notice_since.as_ref().is_some_and(|(at, _)| at.elapsed() > Duration::from_secs(2)) => {
                app.notice = None;
                notice_since = None;
                dirty = true;
            }
            _ => {}
        }
        if dirty {
            let started = Instant::now();
            execute!(io::stdout(), BeginSynchronizedUpdate)?;
            terminal.draw(|f| ui::draw(f, app, &theme, now, &log_lines))?;
            execute!(io::stdout(), EndSynchronizedUpdate)?;
            io::stdout().flush()?;
            perf.record("ui.frame", started.elapsed());
            if let Some(at) = input_at.take() {
                perf.record("ui.input", at.elapsed());
            }
            dirty = false;
        }

        // Wait up to 100 ms for input, then take everything already queued
        // before drawing again: a held key or a wheel burst costs one frame,
        // not one per event (capped so a flood can't starve the draw).
        let mut wait = Duration::from_millis(100);
        let mut batch = 0;
        while batch < MAX_EVENTS_PER_FRAME && event::poll(wait)? {
            wait = Duration::ZERO;
            batch += 1;
            let event = event::read()?;
            input_at.get_or_insert_with(Instant::now);
            let action = match event {
                Event::Key(k) => {
                    let held = held_after(app.held, &k);
                    if held != app.held {
                        app.held = held;
                        dirty = true;
                    }
                    match k.code {
                        KeyCode::Modifier(_) => None,
                        _ if k.kind == KeyEventKind::Release => None,
                        _ => map_key(k.code, k.modifiers).map(|key| app.on_key(key)),
                    }
                }
                // Switching away mid-press would leave a modifier stuck.
                Event::FocusLost => {
                    if app.held != Held::default() {
                        app.held = Held::default();
                        dirty = true;
                    }
                    None
                }
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
                    // What this session's timings were, even if it was short.
                    perf.report(Instant::now());
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
        if !dirty {
            // Nothing to show for it (a drag, a move over the same target).
            input_at = None;
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
        Action::ReloadReviews => collector.reload_reviews(),
        Action::LoadReviewPeople => collector.refresh_people(),
        Action::Open(v) => {
            // Every worktree, hidden ones too, so a workspace in a hidden
            // nested worktree isn't taken for its repo's.
            let roots = app.snapshot.repos.iter().map(|r| r.path.clone())
                .chain(app.snapshot.worktree_paths.iter().cloned())
                .chain(app.snapshot.groups.iter().flat_map(|g| g.worktrees.iter().map(|w| w.worktree.path.clone())))
                .collect();
            launcher::open(*v, roots, &app.settings, launch_tx.clone())
        }
        Action::OpenUrl(url) => launcher::open_url(url, launch_tx.clone()),
        Action::SettingsChanged => {
            *theme = theme::Theme::load(&app.settings.theme);
            collector.update_settings(app.settings.clone());
            if std::mem::take(&mut app.review_fetch_due) {
                collector.refresh_reviews();
            }
        }
        Action::Create(job, repo, req) => ops::create(app.settings.clone(), *repo, *req, job, launch_tx.clone()),
        Action::Delete(job, v, delete_branch) => ops::delete(*v, delete_branch, job, launch_tx.clone()),
        Action::AddRepo(path) => ops::add_repo(path, launch_tx.clone()),
        Action::ReposChanged => collector.refresh(false),
        Action::EditScript(repo_id, kind) => {
            let path = match kind.create(&repo_id) {
                Ok(p) => p,
                Err(e) => {
                    am_core::warn!("scripts", "Couldn't create the {} for {repo_id}: {e}", kind.label());
                    app.popup = Some(format!("✗ Couldn't create the {}: {e}", kind.label()));
                    return Ok(false);
                }
            };
            am_core::info!("scripts", "Editing {} {}", kind.label(), path.display());
            let shown = am_core::hooks_installer::tilde(&path);
            let ide = am_core::launch::IdeChoice::from_setting(&app.settings.ide);
            if !ide.is_terminal() {
                launcher::open_file(path, ide, app.settings.ide_command.clone(), launch_tx.clone());
            } else {
                edit_file(terminal, mouse_before, &path)?;
            }
            if let Some(e) = &mut app.settings_editor {
                let label = kind.label();
                let label = format!("{}{}", label[..1].to_uppercase(), &label[1..]);
                e.refresh_repos(Some((true, format!("{label}: {shown}"))));
            }
        }
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
        Action::RunReviewScript { job, repo_id, worktree_path } => run_review_script(app, job, repo_id, worktree_path, launch_tx),
        Action::AddReviewScriptAndPull(job, repo, req) => {
            let repo_id = repo.id.clone();
            ops::create(app.settings.clone(), *repo, *req, job, launch_tx.clone());
            match ScriptKind::Review.create(&repo_id) {
                Ok(path) => {
                    if let Some(at) = modified(&path) {
                        app.jobs.await_save(job, path, at);
                    }
                }
                Err(e) => {
                    am_core::warn!("scripts", "Couldn't create the review script for {repo_id}: {e}");
                    app.jobs.finish(job, Err(format!("Couldn't create the review script: {e}")), Instant::now());
                    app.notice = Some(format!("Couldn't create the review script: {e}"));
                    return Ok(false);
                }
            }
            return dispatch(Action::EditScript(repo_id, ScriptKind::Review), terminal, app, collector, theme, launch_tx);
        }
        Action::RunScript { job, repo_id, worktree_path } => {
            let in_window = app.settings.startup_script_in_window();
            ops::run_script(ScriptKind::Startup, repo_id, worktree_path, in_window, Vec::new(), job, launch_tx.clone());
        }
        Action::EditSettingsFile => {
            let ide = am_core::launch::IdeChoice::from_setting(&app.settings.ide);
            if !ide.is_terminal() {
                launcher::open_file(am_core::paths::settings_path(), ide, app.settings.ide_command.clone(), launch_tx.clone());
                return Ok(false);
            }
            edit_file(terminal, mouse_before, &am_core::paths::settings_path())?;
            app.set_settings(settings::load());
            *theme = theme::Theme::load(&app.settings.theme);
            collector.update_settings(app.settings.clone());
        }
        Action::OpenReviewWindow => launcher::open_review_window(launch_tx.clone()),
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
                    if ide.is_terminal() {
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

/// Run a "pull + run script" job's review script in its worktree, with the
/// PR's environment plus `AM_WORKTREE_PATH`, in a new terminal window (so an
/// agent it starts shows its own UI and can be talked to), or in the jobs
/// pane when no window can be opened.
fn run_review_script(app: &mut App, job: u64, repo_id: String, worktree_path: String, tx: &std::sync::mpsc::Sender<launcher::Msg>) {
    let mut env = app.jobs.get(job).and_then(|j| j.review_script()).map(<[_]>::to_vec).unwrap_or_default();
    env.push(("AM_WORKTREE_PATH".into(), worktree_path.clone()));
    app.jobs.resume(job);
    info!("tui", "Running the review script for {repo_id} in {worktree_path}");
    ops::run_script(ScriptKind::Review, repo_id, worktree_path, true, env, job, tx.clone());
}

fn modified(path: &std::path::Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn settings_file_mtime() -> Option<std::time::SystemTime> {
    std::fs::metadata(am_core::paths::settings_path()).and_then(|m| m.modified()).ok()
}

/// Hand the terminal to $EDITOR (default `vi`) for a file (settings.json, a
/// startup script, debug.log), then take it back.
fn edit_file(terminal: &mut ratatui::DefaultTerminal, mouse: bool, path: &std::path::Path) -> io::Result<()> {
    let editor = std::env::var("VISUAL").or_else(|_| std::env::var("EDITOR")).unwrap_or_else(|_| "vi".into());
    set_mouse(false);
    set_keyboard(false);
    ratatui::restore();
    // $EDITOR may carry arguments (e.g. `code --wait`), so run it via the shell.
    let status = std::process::Command::new("sh").arg("-c").arg(format!("{editor} \"$1\"")).arg("sh").arg(path).status();
    *terminal = ratatui::init();
    set_mouse(mouse);
    set_keyboard(true);
    terminal.clear()?;
    if let Err(e) = status {
        am_core::warn!("tui", "Failed to run editor {editor} on {}: {e}", path.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::KeyEventState;

    fn ev(code: KeyCode, mods: KeyModifiers, kind: KeyEventKind) -> KeyEvent {
        KeyEvent::new_with_kind_and_state(code, mods, kind, KeyEventState::NONE)
    }

    #[test]
    fn modifier_keys_held_on_their_own_are_tracked() {
        let shift = KeyCode::Modifier(ModifierKeyCode::LeftShift);
        let ctrl = KeyCode::Modifier(ModifierKeyCode::RightControl);
        let h = held_after(Held::default(), &ev(shift, KeyModifiers::SHIFT, KeyEventKind::Press));
        assert_eq!(h, Held { shift: true, ctrl: false });
        // `V` (an alternate key) arrives without SHIFT: shift stays held.
        let h = held_after(h, &ev(KeyCode::Char('V'), KeyModifiers::NONE, KeyEventKind::Press));
        assert!(h.shift);
        let h = held_after(h, &ev(shift, KeyModifiers::SHIFT, KeyEventKind::Release));
        assert_eq!(h, Held::default());
        let h = held_after(h, &ev(ctrl, KeyModifiers::CONTROL, KeyEventKind::Press));
        assert!(h.ctrl);
        // A key without Ctrl means it was let go unseen.
        assert!(!held_after(h, &ev(KeyCode::Char('j'), KeyModifiers::NONE, KeyEventKind::Press)).ctrl);
    }
}
