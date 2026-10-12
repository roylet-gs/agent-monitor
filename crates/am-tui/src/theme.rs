//! Semantic colour tokens. Widgets ask for "faint" or "status(waiting)", never
//! a raw colour, so a theme is just one table. The tables live in
//! `am_core::themes` (built-ins plus `themes/<name>.json`); this turns one into
//! ratatui colours. `ansi` (the default) uses only the 16 terminal colours and
//! therefore follows the user's terminal palette.

use am_core::status::Status;
use am_core::themes::{self, ThemeColor, ThemeSpec, Token};
use ratatui::style::{Color, Modifier, Style};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Selection {
    /// Reverse video (the `ansi` theme: a blue background could collide with
    /// blue text).
    Reverse,
    Bg(Color),
}

#[derive(Debug, Clone)]
pub struct Theme {
    pub name: String,
    pub text: Color,
    pub faint: Color,
    pub accent: Color,
    pub border: Color,
    pub border_focus: Color,
    pub selection: Selection,
    /// Painted behind everything when set; `None` keeps the terminal's.
    pub background: Option<Color>,
    pub green: Color,
    pub yellow: Color,
    pub red: Color,
    pub blue: Color,
    pub magenta: Color,
    pub teal: Color,
}

/// The named ratatui variants for the 16 ANSI indexes, so `ansi` renders
/// exactly as it always has.
const ANSI: [Color; 16] = [
    Color::Black,
    Color::Red,
    Color::Green,
    Color::Yellow,
    Color::Blue,
    Color::Magenta,
    Color::Cyan,
    Color::Gray,
    Color::DarkGray,
    Color::LightRed,
    Color::LightGreen,
    Color::LightYellow,
    Color::LightBlue,
    Color::LightMagenta,
    Color::LightCyan,
    Color::White,
];

pub fn to_color(c: ThemeColor) -> Color {
    match c {
        ThemeColor::Default | ThemeColor::Reverse => Color::Reset,
        ThemeColor::Ansi(i) => ANSI.get(i as usize).copied().unwrap_or(Color::Indexed(i)),
        ThemeColor::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

impl Theme {
    /// Build from a resolved spec (every token set; see `themes::load`).
    pub fn from_spec(spec: &ThemeSpec) -> Theme {
        let c = |t: Token| to_color(spec.get(t));
        Theme {
            name: spec.name.clone(),
            text: c(Token::Text),
            faint: c(Token::Faint),
            accent: c(Token::Accent),
            border: c(Token::Border),
            border_focus: c(Token::BorderFocus),
            selection: match spec.get(Token::Selection) {
                ThemeColor::Reverse | ThemeColor::Default => Selection::Reverse,
                other => Selection::Bg(to_color(other)),
            },
            background: match spec.get(Token::Background) {
                ThemeColor::Default | ThemeColor::Reverse => None,
                other => Some(to_color(other)),
            },
            green: c(Token::Green),
            yellow: c(Token::Yellow),
            red: c(Token::Red),
            blue: c(Token::Blue),
            magenta: c(Token::Magenta),
            teal: c(Token::Teal),
        }
    }

    /// A built-in or custom theme by name; anything unreadable falls back to
    /// `ansi` (logged) so a bad file never blocks the dashboard.
    pub fn load(name: &str) -> Theme {
        match themes::load(name) {
            Ok(spec) => Theme::from_spec(&spec),
            Err(e) => {
                am_core::warn!("theme", "Theme '{name}' unavailable ({e}); using ansi");
                Theme::ansi()
            }
        }
    }

    pub fn ansi() -> Theme {
        Theme::from_spec(&themes::builtin("ansi").expect("ansi is built in"))
    }

    pub fn status(&self, s: Option<Status>) -> Color {
        match s {
            Some(Status::Executing) => self.green,
            Some(Status::Planning) => self.blue,
            Some(Status::Waiting) => self.yellow,
            Some(Status::Delegating) => self.green,
            Some(Status::Done) => self.teal,
            Some(Status::Idle) | None => self.faint,
        }
    }

    pub fn fg(&self, c: Color) -> Style {
        Style::new().fg(c)
    }

    pub fn faint(&self) -> Style {
        self.fg(self.faint)
    }

    pub fn bold(&self, c: Color) -> Style {
        self.fg(c).add_modifier(Modifier::BOLD)
    }

    /// Selected row: a tinted background, or reverse video.
    pub fn selected(&self) -> Style {
        match self.selection {
            Selection::Reverse => Style::new().add_modifier(Modifier::REVERSED),
            Selection::Bg(c) => Style::new().bg(c),
        }
    }

    /// Text on the theme background: fills the screen and every overlay.
    pub fn base(&self) -> Style {
        let s = Style::new().fg(self.text);
        match self.background {
            Some(bg) => s.bg(bg),
            None => s,
        }
    }
}

/// Plain-Unicode glyphs (no Nerd Font needed).
pub fn status_glyph(s: Option<Status>) -> &'static str {
    match s {
        Some(Status::Executing) => "●",
        Some(Status::Planning) => "◇",
        Some(Status::Waiting) => "◐",
        // Half-filled like waiting, but green: the main turn is idle while
        // subagents work, so nothing is needed from the user.
        Some(Status::Delegating) => "◑",
        Some(Status::Done) => "✓",
        Some(Status::Idle) => "○",
        None => "·",
    }
}

pub fn status_label(s: Option<Status>) -> &'static str {
    s.map(Status::as_str).unwrap_or("no agent")
}
