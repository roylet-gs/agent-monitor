//! `am-tui` — native dashboard (Phase 1: read-only).
//!
//! Runs on the alternate screen and draws only when data, input or the
//! clock-second changes. Every frame is wrapped in synchronized-output
//! escapes (DEC mode 2026) so terminals that support it present it atomically.

mod app;
mod data;
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

    let theme = theme::Theme::by_name(&settings.theme);
    let collector = data::Collector::spawn(settings.clone());
    let mut app = App::new(settings);

    // ratatui::init enables raw mode + alt screen and installs a panic hook
    // that restores the terminal before printing the panic.
    let mut terminal = ratatui::init();
    let result = run(&mut terminal, &mut app, &theme, &collector);
    ratatui::restore();
    collector.shutdown();
    result
}

fn run(terminal: &mut ratatui::DefaultTerminal, app: &mut App, theme: &theme::Theme, collector: &data::Collector) -> io::Result<()> {
    let mut dirty = true;
    let mut last_second = am_core::timefmt::now_unix();
    let mut notice_since: Option<Instant> = None;
    loop {
        while let Some(snapshot) = collector.try_recv() {
            app.apply(snapshot);
            dirty = true;
        }
        let now = am_core::timefmt::now_unix();
        if now != last_second {
            // Elapsed/age labels tick once a second; the diff renderer only
            // rewrites the cells that actually changed.
            last_second = now;
            dirty = true;
        }
        if app.notice.is_some() && notice_since.get_or_insert_with(Instant::now).elapsed() > Duration::from_secs(2) {
            app.notice = None;
            notice_since = None;
            dirty = true;
        }
        if dirty {
            execute!(io::stdout(), BeginSynchronizedUpdate)?;
            terminal.draw(|f| ui::draw(f, app, theme, now))?;
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
                            Action::None => {}
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
