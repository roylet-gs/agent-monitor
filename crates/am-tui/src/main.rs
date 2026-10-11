//! `am-tui` — native dashboard (Phase 1: read-only).
//!
//! Runs on the alternate screen and draws only when data, input or the
//! clock-second changes. Every frame is wrapped in synchronized-output
//! escapes (DEC mode 2026) so terminals that support it present it atomically.

mod app;
mod data;
mod launcher;
mod modals;
mod ops;
mod settings_ui;
mod setup;
mod theme;
mod ui;

use std::io::{self, Write};
use std::time::{Duration, Instant};

use am_core::{info, settings};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::crossterm::terminal::{BeginSynchronizedUpdate, EndSynchronizedUpdate};
use ratatui::crossterm::execute;

use app::{Action, App, Key};

fn map_key(code: KeyCode, mods: KeyModifiers) -> Option<Key> {
    let ctrl = mods.contains(KeyModifiers::CONTROL);
    Some(match code {
        KeyCode::Char('c') if ctrl => Key::CtrlC,
        KeyCode::Char('d') if ctrl => Key::CtrlD,
        KeyCode::Char('u') if ctrl => Key::CtrlU,
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
    let first_run = setup::needed(&settings);
    let ide = settings.ide.clone();
    let mut app = App::new(settings);
    if first_run {
        app.setup = Some(setup::Setup::new(Vec::new(), &ide));
    }

    // ratatui::init enables raw mode + alt screen and installs a panic hook
    // that restores the terminal before printing the panic.
    let mut terminal = ratatui::init();
    let result = run(&mut terminal, &mut app, &collector);
    ratatui::restore();
    collector.shutdown();
    result
}

fn run(terminal: &mut ratatui::DefaultTerminal, app: &mut App, collector: &data::Collector) -> io::Result<()> {
    let mut theme = theme::Theme::by_name(&app.settings.theme);
    let (launch_tx, launch_rx) = std::sync::mpsc::channel();
    let mut popup_since: Option<Instant> = None;
    let mut log_lines: Vec<String> = Vec::new();
    let mut dirty = true;
    let mut last_second = am_core::timefmt::now_unix();
    let mut notice_since: Option<Instant> = None;
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
                launcher::Msg::Created(repo, req, result) => {
                    use am_core::worktrees::CreateError;
                    app.modal = None;
                    match result {
                        Ok(w) => {
                            app.notice = Some(format!("Created {}", w.branch));
                            app.pending_select = Some(w.id.clone());
                            let _ = &w.id;
                            if am_core::worktrees::has_script(&repo.id) {
                                app.modal = Some(modals::Modal::RunScript { repo_id: repo.id.clone(), worktree_path: w.path.clone() });
                            }
                            collector.refresh(false);
                        }
                        Err(CreateError::BranchExists { local, remote, .. }) if local || remote => {
                            app.modal = Some(modals::Modal::BranchExists { repo: *repo, req: *req, local, remote });
                        }
                        Err(e) => {
                            app.popup = Some(format!("✗ {e}"));
                            popup_since = Some(Instant::now());
                        }
                    }
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
                        collector.refresh(false);
                    }
                    Err(e) => app.modal = Some(modals::Modal::AddRepo { path, error: Some(e) }),
                },
                launcher::Msg::Deleted(label, result) => {
                    app.modal = None;
                    match result {
                        Ok(warnings) if warnings.is_empty() => app.notice = Some(format!("Deleted {label}")),
                        Ok(warnings) => {
                            app.popup = Some(format!("Deleted {label}, with warnings:\n\n{}", warnings.join("\n")));
                            popup_since = Some(Instant::now());
                        }
                        Err(e) => {
                            app.popup = Some(format!("✗ {e}"));
                            popup_since = Some(Instant::now());
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
            match event::read()? {
                Event::Key(k) if k.kind != KeyEventKind::Release => {
                    if let Some(key) = map_key(k.code, k.modifiers) {
                        match app.on_key(key) {
                            Action::Quit => return Ok(()),
                            Action::Refresh => collector.refresh(true),
                            Action::Open(v) => launcher::open(*v, &app.settings, launch_tx.clone()),
                            Action::OpenTerminal(v) => launcher::open_terminal(*v, launch_tx.clone()),
                            Action::OpenClaude(v) => launcher::open_claude(*v, launch_tx.clone()),
                            Action::OpenUrl(url) => launcher::open_url(url, launch_tx.clone()),
                            Action::SettingsChanged => {
                                theme = theme::Theme::by_name(&app.settings.theme);
                                collector.update_settings(app.settings.clone());
                            }
                            Action::Create(repo, req) => ops::create(app.settings.clone(), *repo, *req, launch_tx.clone()),
                            Action::Delete(v, delete_branch) => ops::delete(*v, delete_branch, launch_tx.clone()),
                            Action::AddRepo(path) => ops::add_repo(path, launch_tx.clone()),
                            Action::InstallHooks => {
                                let r = am_core::hooks_installer::install_global_hooks()
                                    .map(|bin| bin.map(|b| b.display().to_string()))
                                    .map_err(|e| e.to_string());
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
                            Action::RunScript { repo_id, worktree_path } => {
                                run_script(terminal, &repo_id, &worktree_path)?;
                            }
                            Action::EditSettingsFile => {
                                edit_settings_file(terminal)?;
                                app.set_settings(settings::load());
                                theme = theme::Theme::by_name(&app.settings.theme);
                                collector.update_settings(app.settings.clone());
                            }
                            Action::None => {}
                        }
                        if app.show_log && log_lines.is_empty() {
                            log_lines = tail_log(200);
                        }
                        dirty = true;
                    }
                }
                Event::Resize(..) => dirty = true,
                _ => {}
            }
        }
    }
}

/// macOS system sound by name; fire-and-forget (port of src/lib/audio.ts).
fn play_sound(name: &str) {
    if !name.chars().all(|c| c.is_ascii_alphanumeric()) {
        return;
    }
    let path = format!("/System/Library/Sounds/{name}.aiff");
    let _ = std::process::Command::new("afplay")
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
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
fn edit_settings_file(terminal: &mut ratatui::DefaultTerminal) -> io::Result<()> {
    let path = am_core::paths::settings_path();
    let editor = std::env::var("VISUAL").or_else(|_| std::env::var("EDITOR")).unwrap_or_else(|_| "vi".into());
    ratatui::restore();
    // $EDITOR may carry arguments (e.g. `code --wait`), so run it via the shell.
    let status = std::process::Command::new("sh").arg("-c").arg(format!("{editor} \"$1\"")).arg("sh").arg(&path).status();
    *terminal = ratatui::init();
    terminal.clear()?;
    if let Err(e) = status {
        am_core::warn!("settings", "Failed to run editor {editor}: {e}");
    }
    Ok(())
}

/// Run a repo's startup script in the real terminal (the TUI steps aside),
/// then wait for Enter so the output can be read before returning.
fn run_script(terminal: &mut ratatui::DefaultTerminal, repo_id: &str, cwd: &str) -> io::Result<()> {
    let script = am_core::worktrees::script_path(repo_id);
    ratatui::restore();
    println!("\x1b[1m▶ Running startup script\x1b[0m {}\n  in {cwd}\n", script.display());
    let status = std::process::Command::new("/bin/sh").arg(&script).current_dir(cwd).status();
    match status {
        Ok(s) if s.success() => println!("\n\x1b[32m✓ Script finished.\x1b[0m Press Enter to return to am…"),
        Ok(s) => println!("\n\x1b[31m✗ Script exited with {s}.\x1b[0m Press Enter to return to am…"),
        Err(e) => println!("\n\x1b[31m✗ Couldn't run script: {e}\x1b[0m Press Enter to return to am…"),
    }
    let mut line = String::new();
    let _ = io::stdin().read_line(&mut line);
    *terminal = ratatui::init();
    terminal.clear()?;
    Ok(())
}
