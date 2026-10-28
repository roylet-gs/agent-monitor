//! Theme builder (Settings → "Theme builder…"). Pure state: it starts from a
//! base theme, keeps per-token overrides, and hands back a `ThemeSpec` to save.
//! The settings editor does the file I/O. While it is open the whole dashboard
//! is drawn with the draft (`theme()`), so every change previews live.

use std::collections::BTreeMap;

use am_core::themes::{self, ThemeColor, ThemeSpec, Token};

use crate::app::Key;
use crate::theme::Theme;

/// Swatch grid width in the palette picker.
pub const COLS: usize = 8;

/// Curated swatches: light / mid / dark / deep shades of red, orange, yellow,
/// green, teal, blue, purple, pink, then a grey ramp.
#[rustfmt::skip]
const CURATED: [u32; 40] = [
    0xfca5a5, 0xfdba74, 0xfde047, 0x86efac, 0x5eead4, 0x93c5fd, 0xc4b5fd, 0xf9a8d4,
    0xef4444, 0xf97316, 0xeab308, 0x22c55e, 0x14b8a6, 0x3b82f6, 0x8b5cf6, 0xec4899,
    0xb91c1c, 0xc2410c, 0xa16207, 0x15803d, 0x0f766e, 0x1d4ed8, 0x6d28d9, 0xbe185d,
    0x7f1d1d, 0x7c2d12, 0x713f12, 0x14532d, 0x134e4a, 0x1e3a8a, 0x4c1d95, 0x831843,
    0xffffff, 0xe5e7eb, 0x9ca3af, 0x6b7280, 0x4b5563, 0x374151, 0x1f2937, 0x000000,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    Tokens,
    Picker { cursor: usize },
    Hex(String),
    Name(String),
}

#[derive(Debug, PartialEq, Eq)]
pub enum BuilderOutcome {
    Stay,
    Close,
    Save(ThemeSpec),
}

pub struct ThemeBuilder {
    pub name: String,
    pub base: String,
    /// Tokens this theme sets itself; the rest come from `base`.
    pub overrides: BTreeMap<Token, ThemeColor>,
    pub cursor: usize,
    pub mode: Mode,
    pub dirty: bool,
    pub error: Option<String>,
    confirm_discard: bool,
    /// Every theme it can build on, resolved up front so the builder never
    /// touches the disk.
    catalog: Vec<ThemeSpec>,
}

impl ThemeBuilder {
    /// `current` is the active theme; `stored` is its file when it's custom
    /// (then it's edited in place), otherwise the builder starts a copy.
    pub fn open(current: &str, stored: Option<ThemeSpec>, catalog: Vec<ThemeSpec>) -> ThemeBuilder {
        let (name, base, overrides) = match stored {
            Some(s) => (s.name.clone(), s.base.clone().unwrap_or_else(|| "ansi".into()), s.colors),
            None => {
                let base = if themes::is_builtin(current) {
                    themes::builtin(current).map(|t| t.name).unwrap_or_else(|| "ansi".into())
                } else {
                    "ansi".to_string()
                };
                (format!("{base}-custom"), base, BTreeMap::new())
            }
        };
        let catalog = catalog.into_iter().filter(|t| t.name != name).collect();
        ThemeBuilder { name, base, overrides, cursor: 0, mode: Mode::Tokens, dirty: false, error: None, confirm_discard: false, catalog }
    }

    pub fn token(&self) -> Token {
        Token::ALL[self.cursor]
    }

    fn base_spec(&self) -> ThemeSpec {
        self.catalog
            .iter()
            .find(|t| t.name == self.base)
            .cloned()
            .or_else(|| themes::builtin("ansi"))
            .expect("ansi is built in")
    }

    /// The draft, fully resolved. In the picker the hovered swatch previews
    /// on the current token before it's chosen.
    pub fn resolved(&self) -> ThemeSpec {
        let mut spec = self.base_spec();
        spec.name = self.name.clone();
        spec.base = Some(self.base.clone());
        for (t, c) in &self.overrides {
            spec.colors.insert(*t, *c);
        }
        if let Mode::Picker { cursor } = self.mode
            && let Some(c) = self.palette().get(cursor)
        {
            spec.colors.insert(self.token(), *c);
        }
        spec
    }

    pub fn theme(&self) -> Theme {
        Theme::from_spec(&self.resolved())
    }

    /// Effective colour of a token and whether this theme sets it itself.
    pub fn color(&self, t: Token) -> (ThemeColor, bool) {
        match self.overrides.get(&t) {
            Some(c) => (*c, true),
            None => (self.base_spec().get(t), false),
        }
    }

    /// Swatches for the current token: the 16 ANSI colours, the curated grid,
    /// then this draft's own colours (plus `reverse`/`default` where they
    /// make sense), each row `COLS` wide.
    pub fn palette(&self) -> Vec<ThemeColor> {
        let mut out: Vec<ThemeColor> = (0..16).map(ThemeColor::Ansi).collect();
        out.extend(CURATED.iter().map(|&h| ThemeColor::Rgb((h >> 16) as u8, (h >> 8) as u8, h as u8)));
        let mut own: Vec<ThemeColor> = Vec::new();
        match self.token() {
            Token::Selection => own.push(ThemeColor::Reverse),
            Token::Background | Token::Text => own.push(ThemeColor::Default),
            _ => {}
        }
        for t in Token::ALL {
            let c = self.color(t).0;
            if matches!(c, ThemeColor::Rgb(..)) && !out.contains(&c) && !own.contains(&c) && own.len() < COLS {
                own.push(c);
            }
        }
        out.extend(own);
        out
    }

    fn set(&mut self, c: ThemeColor) {
        let t = self.token();
        if !themes::allowed(t, c) {
            self.error = Some(format!("{c} only works for selection"));
            return;
        }
        self.overrides.insert(t, c);
        self.dirty = true;
    }

    pub fn on_key(&mut self, key: Key) -> BuilderOutcome {
        match std::mem::replace(&mut self.mode, Mode::Tokens) {
            Mode::Tokens => self.on_token_key(key),
            Mode::Picker { cursor } => {
                self.on_picker_key(key, cursor);
                BuilderOutcome::Stay
            }
            Mode::Hex(buf) => {
                self.on_hex_key(key, buf);
                BuilderOutcome::Stay
            }
            Mode::Name(buf) => {
                self.on_name_key(key, buf);
                BuilderOutcome::Stay
            }
        }
    }

    fn on_token_key(&mut self, key: Key) -> BuilderOutcome {
        let confirming = std::mem::take(&mut self.confirm_discard);
        self.error = None;
        match key {
            Key::Esc | Key::Char('q') if self.dirty && !confirming => {
                self.confirm_discard = true;
                self.error = Some("Unsaved changes — esc again to discard, w to save".into());
            }
            Key::Esc | Key::Char('q') => return BuilderOutcome::Close,
            k if let Some(to) = crate::nav::step(k, true, self.cursor, Token::ALL.len()) => self.cursor = to,
            Key::Right | Key::Char('l') => self.nudge(1),
            Key::Left | Key::Char('h') => self.nudge(-1),
            Key::Enter | Key::Char(' ') => {
                let cur = self.color(self.token()).0;
                let cursor = self.palette().iter().position(|c| *c == cur).unwrap_or(0);
                self.mode = Mode::Picker { cursor };
            }
            Key::Char('#') => self.mode = Mode::Hex(self.hex_seed()),
            Key::Char('x') => {
                if self.overrides.remove(&self.token()).is_some() {
                    self.dirty = true;
                }
            }
            Key::Char('X') => {
                if !self.overrides.is_empty() {
                    self.overrides.clear();
                    self.dirty = true;
                }
            }
            Key::Char('b') => self.cycle_base(1),
            Key::Char('B') => self.cycle_base(-1),
            Key::Char('n') => self.mode = Mode::Name(self.name.clone()),
            Key::Char('w') | Key::CtrlS => {
                if let Err(e) = themes::validate_name(&self.name) {
                    self.error = Some(format!("{e} (n to rename)"));
                } else {
                    return BuilderOutcome::Save(ThemeSpec {
                        name: self.name.clone(),
                        base: Some(self.base.clone()),
                        colors: self.overrides.clone(),
                    });
                }
            }
            _ => {}
        }
        BuilderOutcome::Stay
    }

    /// ←/→ on a token: step through the palette without opening it.
    fn nudge(&mut self, dir: i64) {
        let pal = self.palette();
        let cur = self.color(self.token()).0;
        let next = match pal.iter().position(|c| *c == cur) {
            Some(i) => (i as i64 + dir).rem_euclid(pal.len() as i64) as usize,
            None if dir > 0 => 0,
            None => pal.len() - 1,
        };
        self.set(pal[next]);
    }

    fn cycle_base(&mut self, dir: i64) {
        let n = self.catalog.len() as i64;
        if n == 0 {
            return;
        }
        let i = self.catalog.iter().position(|t| t.name == self.base).map(|i| i as i64).unwrap_or(-1);
        self.base = self.catalog[(i + dir).rem_euclid(n) as usize].name.clone();
        self.dirty = true;
    }

    fn hex_seed(&self) -> String {
        match self.color(self.token()).0 {
            c @ ThemeColor::Rgb(..) => c.to_string(),
            _ => "#".into(),
        }
    }

    fn on_picker_key(&mut self, key: Key, cursor: usize) {
        let n = self.palette().len();
        let mut cursor = cursor.min(n - 1);
        match key {
            Key::Esc | Key::Char('q') => return,
            Key::Enter | Key::Char(' ') => {
                let c = self.palette()[cursor];
                self.set(c);
                return;
            }
            Key::Char('#') => {
                self.mode = Mode::Hex(self.hex_seed());
                return;
            }
            Key::Right | Key::Char('l') => cursor = (cursor + 1).min(n - 1),
            Key::Left | Key::Char('h') => cursor = cursor.saturating_sub(1),
            Key::Down | Key::Char('j') => cursor = (cursor + COLS).min(n - 1),
            Key::Up | Key::Char('k') => cursor = cursor.saturating_sub(COLS),
            _ => {}
        }
        self.mode = Mode::Picker { cursor };
    }

    fn on_hex_key(&mut self, key: Key, mut buf: String) {
        match key {
            Key::Esc => {
                self.error = None;
                return;
            }
            Key::Enter => match ThemeColor::parse(&buf) {
                Ok(c) if themes::allowed(self.token(), c) => {
                    self.error = None;
                    self.set(c);
                    return;
                }
                Ok(c) => self.error = Some(format!("{c} only works for selection")),
                Err(e) => self.error = Some(e),
            },
            Key::Backspace => {
                buf.pop();
            }
            Key::Char(c) if buf.len() < 24 && !c.is_control() => buf.push(c),
            _ => {}
        }
        self.mode = Mode::Hex(buf);
    }

    fn on_name_key(&mut self, key: Key, mut buf: String) {
        match key {
            Key::Esc => {
                self.error = None;
                return;
            }
            Key::Enter => match themes::validate_name(&buf) {
                Ok(()) => {
                    self.error = None;
                    if buf != self.name {
                        self.name = buf;
                        self.dirty = true;
                    }
                    return;
                }
                Err(e) => self.error = Some(e),
            },
            Key::Backspace => {
                buf.pop();
            }
            Key::Char(c) if buf.len() < 40 && !c.is_control() => buf.push(c),
            _ => {}
        }
        self.mode = Mode::Name(buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> Vec<ThemeSpec> {
        themes::builtin_names().into_iter().filter_map(themes::builtin).collect()
    }

    fn builder() -> ThemeBuilder {
        ThemeBuilder::open("catppuccin-mocha", None, catalog())
    }

    fn type_str(b: &mut ThemeBuilder, s: &str) {
        for c in s.chars() {
            b.on_key(Key::Char(c));
        }
    }

    #[test]
    fn opens_a_copy_of_a_builtin_and_edits_a_custom_in_place() {
        let b = builder();
        assert_eq!((b.name.as_str(), b.base.as_str()), ("catppuccin-mocha-custom", "catppuccin-mocha"));
        assert!(b.overrides.is_empty() && !b.dirty);

        let mut colors = BTreeMap::new();
        colors.insert(Token::Red, ThemeColor::Rgb(1, 2, 3));
        let stored = ThemeSpec { name: "mine".into(), base: Some("nord".into()), colors };
        let b = ThemeBuilder::open("mine", Some(stored), catalog());
        assert_eq!((b.name.as_str(), b.base.as_str()), ("mine", "nord"));
        assert_eq!(b.color(Token::Red), (ThemeColor::Rgb(1, 2, 3), true));
        assert_eq!(b.color(Token::Green), (themes::builtin("nord").unwrap().get(Token::Green), false));
    }

    #[test]
    fn picker_previews_the_hovered_swatch_and_picks_it() {
        let mut b = builder();
        b.cursor = Token::ALL.iter().position(|t| *t == Token::Accent).unwrap();
        b.on_key(Key::Enter);
        assert!(matches!(b.mode, Mode::Picker { .. }));
        b.mode = Mode::Picker { cursor: 0 };
        b.on_key(Key::Down); // first curated row: light red
        b.on_key(Key::Down);
        assert_eq!(b.mode, Mode::Picker { cursor: 16 });
        assert_eq!(b.resolved().get(Token::Accent), ThemeColor::Rgb(0xfc, 0xa5, 0xa5), "hover previews");
        assert!(b.overrides.is_empty(), "nothing chosen yet");

        b.on_key(Key::Esc);
        assert_eq!(b.resolved().get(Token::Accent), themes::builtin("catppuccin-mocha").unwrap().get(Token::Accent));

        b.on_key(Key::Enter);
        b.mode = Mode::Picker { cursor: 17 };
        b.on_key(Key::Enter);
        assert_eq!(b.mode, Mode::Tokens);
        assert_eq!(b.color(Token::Accent), (ThemeColor::Rgb(0xfd, 0xba, 0x74), true));
        assert!(b.dirty);
    }

    #[test]
    fn hex_entry_validates() {
        let mut b = builder();
        b.on_key(Key::Char('#'));
        assert_eq!(b.mode, Mode::Hex("#cdd6f4".into()), "seeded with the current colour");
        for _ in 0..6 {
            b.on_key(Key::Backspace);
        }
        type_str(&mut b, "zz");
        b.on_key(Key::Enter);
        assert!(b.error.is_some() && matches!(b.mode, Mode::Hex(_)));
        b.on_key(Key::Backspace);
        b.on_key(Key::Backspace);
        type_str(&mut b, "f0a");
        b.on_key(Key::Enter);
        assert_eq!(b.mode, Mode::Tokens);
        assert_eq!(b.color(Token::Text), (ThemeColor::Rgb(0xff, 0x00, 0xaa), true));
    }

    #[test]
    fn reverse_is_only_offered_for_selection() {
        let mut b = builder();
        assert!(!b.palette().contains(&ThemeColor::Reverse));
        b.on_key(Key::Char('#'));
        b.mode = Mode::Hex("reverse".into());
        b.on_key(Key::Enter);
        assert!(b.error.is_some());
        b.mode = Mode::Tokens;
        b.cursor = Token::ALL.iter().position(|t| *t == Token::Selection).unwrap();
        assert!(b.palette().contains(&ThemeColor::Reverse));
    }

    #[test]
    fn nudge_clear_and_base_cycling() {
        let mut b = ThemeBuilder::open("ansi", None, catalog());
        b.cursor = Token::ALL.iter().position(|t| *t == Token::Accent).unwrap();
        b.on_key(Key::Right);
        assert_eq!(b.color(Token::Accent), (ThemeColor::Ansi(7), true), "cyan → gray");
        b.on_key(Key::Left);
        b.on_key(Key::Left);
        assert_eq!(b.color(Token::Accent).0, ThemeColor::Ansi(5));
        b.on_key(Key::Char('x'));
        assert_eq!(b.color(Token::Accent), (ThemeColor::Ansi(6), false), "x inherits from the base again");

        b.on_key(Key::Char('#'));
        b.mode = Mode::Hex("#123456".into());
        b.on_key(Key::Enter);
        b.on_key(Key::Char('b'));
        assert_eq!(b.base, "catppuccin-mocha");
        assert_eq!(b.color(Token::Accent).0, ThemeColor::Rgb(0x12, 0x34, 0x56), "overrides survive a base change");
        assert_eq!(b.resolved().get(Token::Red), themes::builtin("catppuccin-mocha").unwrap().get(Token::Red));
        b.on_key(Key::Char('B'));
        assert_eq!(b.base, "ansi");
        b.on_key(Key::Char('X'));
        assert!(b.overrides.is_empty());
    }

    #[test]
    fn save_needs_a_valid_name_and_esc_confirms_discard() {
        let mut b = builder();
        b.on_key(Key::Right);
        b.on_key(Key::Char('n'));
        for _ in 0..40 {
            b.on_key(Key::Backspace);
        }
        type_str(&mut b, "dracula");
        b.on_key(Key::Enter);
        assert!(b.error.is_some(), "built-in names are taken");
        for _ in 0..7 {
            b.on_key(Key::Backspace);
        }
        type_str(&mut b, "neon");
        b.on_key(Key::Enter);
        assert_eq!(b.name, "neon");

        let BuilderOutcome::Save(spec) = b.on_key(Key::Char('w')) else { panic!("expected save") };
        assert_eq!(spec.name, "neon");
        assert_eq!(spec.base.as_deref(), Some("catppuccin-mocha"));
        assert_eq!(spec.colors.len(), 1);

        assert_eq!(b.on_key(Key::Esc), BuilderOutcome::Stay, "first esc warns");
        assert!(b.error.is_some());
        assert_eq!(b.on_key(Key::Esc), BuilderOutcome::Close);
        assert_eq!(builder().on_key(Key::Esc), BuilderOutcome::Close, "clean builder closes at once");
    }
}
