//! Semantic colour tokens. Widgets ask for "faint" or "status(waiting)", never
//! a raw colour, so a theme is just one table. `ansi` (the default) uses only
//! the 16 terminal colours and therefore follows the user's terminal palette.

use am_core::status::Status;
use ratatui::style::{Color, Modifier, Style};

#[derive(Debug, Clone)]
pub struct Theme {
    pub name: &'static str,
    pub text: Color,
    pub faint: Color,
    pub accent: Color,
    pub border: Color,
    pub border_focus: Color,
    pub selection_bg: Color,
    pub green: Color,
    pub yellow: Color,
    pub red: Color,
    pub blue: Color,
    pub magenta: Color,
    pub teal: Color,
}

fn hex(rgb: u32) -> Color {
    Color::Rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

impl Theme {
    pub fn by_name(name: &str) -> Theme {
        match name {
            "catppuccin" | "catppuccin-mocha" => Theme {
                name: "catppuccin-mocha",
                text: hex(0xcdd6f4),
                faint: hex(0x7f849c),
                accent: hex(0x89b4fa),
                border: hex(0x585b70),
                border_focus: hex(0xb4befe),
                selection_bg: hex(0x313244),
                green: hex(0xa6e3a1),
                yellow: hex(0xf9e2af),
                red: hex(0xf38ba8),
                blue: hex(0x89b4fa),
                magenta: hex(0xcba6f7),
                teal: hex(0x94e2d5),
            },
            "tokyonight" => Theme {
                name: "tokyonight",
                text: hex(0xc0caf5),
                faint: hex(0x565f89),
                accent: hex(0x7aa2f7),
                border: hex(0x3b4261),
                border_focus: hex(0x7aa2f7),
                selection_bg: hex(0x283457),
                green: hex(0x9ece6a),
                yellow: hex(0xe0af68),
                red: hex(0xf7768e),
                blue: hex(0x7aa2f7),
                magenta: hex(0xbb9af7),
                teal: hex(0x73daca),
            },
            "gruvbox" => Theme {
                name: "gruvbox",
                text: hex(0xebdbb2),
                faint: hex(0x928374),
                accent: hex(0x83a598),
                border: hex(0x504945),
                border_focus: hex(0xb8bb26),
                selection_bg: hex(0x3c3836),
                green: hex(0xb8bb26),
                yellow: hex(0xfabd2f),
                red: hex(0xfb4934),
                blue: hex(0x83a598),
                magenta: hex(0xd3869b),
                teal: hex(0x8ec07c),
            },
            _ => Theme {
                name: "ansi",
                text: Color::Reset,
                faint: Color::DarkGray,
                accent: Color::Cyan,
                border: Color::DarkGray,
                border_focus: Color::Green,
                selection_bg: Color::Blue,
                green: Color::Green,
                yellow: Color::Yellow,
                red: Color::Red,
                blue: Color::Blue,
                magenta: Color::Magenta,
                teal: Color::Cyan,
            },
        }
    }

    pub fn status(&self, s: Option<Status>) -> Color {
        match s {
            Some(Status::Executing) => self.green,
            Some(Status::Planning) => self.blue,
            Some(Status::Waiting) => self.yellow,
            Some(Status::Delegating) => self.magenta,
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

    /// Selected row: a tinted background, except in the ANSI theme where a
    /// blue background could collide with blue text — it uses reverse video.
    pub fn selected(&self) -> Style {
        if self.name == "ansi" {
            Style::new().add_modifier(Modifier::REVERSED)
        } else {
            Style::new().bg(self.selection_bg)
        }
    }
}

/// Plain-Unicode glyphs (no Nerd Font needed).
pub fn status_glyph(s: Option<Status>) -> &'static str {
    match s {
        Some(Status::Executing) => "●",
        Some(Status::Planning) => "◇",
        Some(Status::Waiting) => "◐",
        Some(Status::Delegating) => "◆",
        Some(Status::Done) => "✓",
        Some(Status::Idle) => "○",
        None => "·",
    }
}

pub fn status_label(s: Option<Status>) -> &'static str {
    s.map(Status::as_str).unwrap_or("no agent")
}
