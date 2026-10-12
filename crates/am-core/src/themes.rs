//! Colour themes: the built-in table plus user themes stored as one JSON file
//! each in `<app dir>/themes/<name>.json`. Shared by the TUI (which turns a
//! resolved theme into ratatui colours) and `am theme`.
//!
//! A theme is a map of semantic tokens to colours. A custom theme names a
//! `base` and only stores the tokens it overrides; anything missing comes from
//! the base, then from `ansi`.
//!
//! ```json
//! { "name": "my-theme", "base": "catppuccin-mocha",
//!   "colors": { "accent": "#f5c2e7", "background": "#1e1e2e" } }
//! ```

mod builtin;

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::{debug, paths, warn};

/// Semantic colour slots. Widgets ask for a token, never a raw colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Token {
    Text,
    Faint,
    Accent,
    Border,
    BorderFocus,
    Selection,
    /// Painted behind the whole UI when set; `default` keeps the terminal's.
    Background,
    Green,
    Yellow,
    Red,
    Blue,
    Magenta,
    Teal,
}

impl Token {
    pub const ALL: [Token; 13] = [
        Token::Text,
        Token::Faint,
        Token::Accent,
        Token::Border,
        Token::BorderFocus,
        Token::Selection,
        Token::Background,
        Token::Green,
        Token::Yellow,
        Token::Red,
        Token::Blue,
        Token::Magenta,
        Token::Teal,
    ];

    /// JSON key in a theme file.
    pub fn key(self) -> &'static str {
        match self {
            Token::Text => "text",
            Token::Faint => "faint",
            Token::Accent => "accent",
            Token::Border => "border",
            Token::BorderFocus => "borderFocus",
            Token::Selection => "selection",
            Token::Background => "background",
            Token::Green => "green",
            Token::Yellow => "yellow",
            Token::Red => "red",
            Token::Blue => "blue",
            Token::Magenta => "magenta",
            Token::Teal => "teal",
        }
    }

    pub fn from_key(key: &str) -> Option<Token> {
        Token::ALL.into_iter().find(|t| t.key() == key)
    }

    /// What the token colours on the dashboard (builder + `am theme show`).
    pub fn describe(self) -> &'static str {
        match self {
            Token::Text => "body text",
            Token::Faint => "hints, branch names, idle",
            Token::Accent => "section titles, spinner",
            Token::Border => "pane borders",
            Token::BorderFocus => "focused border, selection bar",
            Token::Selection => "selected row background",
            Token::Background => "whole-screen background",
            Token::Green => "executing, checks passed",
            Token::Yellow => "waiting, warnings",
            Token::Red => "failures, danger",
            Token::Blue => "planning, tickets",
            Token::Magenta => "projects, merged PRs",
            Token::Teal => "done",
        }
    }
}

/// The 16 ANSI colours, by the names theme files use (index = ANSI number).
pub const ANSI_NAMES: [&str; 16] = [
    "black",
    "red",
    "green",
    "yellow",
    "blue",
    "magenta",
    "cyan",
    "gray",
    "darkgray",
    "lightred",
    "lightgreen",
    "lightyellow",
    "lightblue",
    "lightmagenta",
    "lightcyan",
    "white",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThemeColor {
    /// The terminal's own foreground/background.
    Default,
    /// One of the 16 terminal palette colours (follows the user's terminal).
    Ansi(u8),
    Rgb(u8, u8, u8),
    /// Reverse video; only meaningful for `selection`.
    Reverse,
}

impl ThemeColor {
    /// Accepts `#rgb`, `#rrggbb`, an ANSI name (`darkgray`, `dark-gray`,
    /// `grey`…), `default`/`reset`, or `reverse`.
    pub fn parse(s: &str) -> Result<ThemeColor, String> {
        let t = s.trim().to_ascii_lowercase();
        if let Some(hex) = t.strip_prefix('#') {
            return parse_hex(hex).ok_or_else(|| format!("'{s}' is not a #rgb or #rrggbb colour"));
        }
        let name: String = t.chars().filter(|c| *c != '-' && *c != '_' && *c != ' ').collect();
        let name = name.replace("grey", "gray");
        match name.as_str() {
            "default" | "reset" | "none" => Ok(ThemeColor::Default),
            "reverse" | "reversed" => Ok(ThemeColor::Reverse),
            _ => ANSI_NAMES
                .iter()
                .position(|n| *n == name)
                .map(|i| ThemeColor::Ansi(i as u8))
                .ok_or_else(|| format!("'{s}' is not a colour (use #rrggbb, an ANSI name, or default)")),
        }
    }

    pub fn rgb(self) -> Option<(u8, u8, u8)> {
        match self {
            ThemeColor::Rgb(r, g, b) => Some((r, g, b)),
            _ => None,
        }
    }
}

fn parse_hex(hex: &str) -> Option<ThemeColor> {
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let n = |s: &str| u8::from_str_radix(s, 16).ok();
    match hex.len() {
        6 => Some(ThemeColor::Rgb(n(&hex[0..2])?, n(&hex[2..4])?, n(&hex[4..6])?)),
        3 => {
            let d = |i: usize| n(&hex[i..i + 1]).map(|v| v * 17);
            Some(ThemeColor::Rgb(d(0)?, d(1)?, d(2)?))
        }
        _ => None,
    }
}

impl fmt::Display for ThemeColor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ThemeColor::Default => f.write_str("default"),
            ThemeColor::Reverse => f.write_str("reverse"),
            ThemeColor::Ansi(i) => f.write_str(ANSI_NAMES.get(*i as usize).copied().unwrap_or("default")),
            ThemeColor::Rgb(r, g, b) => write!(f, "#{r:02x}{g:02x}{b:02x}"),
        }
    }
}

/// `reverse` only makes sense as a selection style.
pub fn allowed(token: Token, color: ThemeColor) -> bool {
    color != ThemeColor::Reverse || token == Token::Selection
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeSpec {
    pub name: String,
    pub base: Option<String>,
    /// Overrides for a custom theme; every token once resolved.
    pub colors: BTreeMap<Token, ThemeColor>,
}

impl ThemeSpec {
    /// Colour for `token`. Unset tokens read as `default`; call on a resolved
    /// spec (see [`load`]) to get the effective colour.
    pub fn get(&self, token: Token) -> ThemeColor {
        self.colors.get(&token).copied().unwrap_or(ThemeColor::Default)
    }

    /// The theme file contents: name, base, and only the stored overrides.
    pub fn to_json(&self) -> String {
        let colors: serde_json::Map<String, serde_json::Value> =
            Token::ALL.iter().filter_map(|t| self.colors.get(t).map(|c| (t.key().to_string(), c.to_string().into()))).collect();
        let mut obj = serde_json::Map::new();
        obj.insert("name".into(), self.name.clone().into());
        if let Some(b) = &self.base {
            obj.insert("base".into(), b.clone().into());
        }
        obj.insert("colors".into(), colors.into());
        serde_json::to_string_pretty(&serde_json::Value::Object(obj)).unwrap_or_default() + "\n"
    }

    /// Lenient parse of a theme file: unknown tokens and bad colours are
    /// skipped (and logged) so they fall back to the base instead of failing
    /// the whole theme. `name` comes from the file name, not the contents.
    pub fn from_json(name: &str, text: &str) -> Result<ThemeSpec, String> {
        let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("invalid JSON: {e}"))?;
        let obj = v.as_object().ok_or("a theme file must be a JSON object")?;
        let base = obj.get("base").and_then(|b| b.as_str()).map(str::to_string).filter(|b| !b.is_empty());
        let mut colors = BTreeMap::new();
        if let Some(map) = obj.get("colors").and_then(|c| c.as_object()) {
            for (k, v) in map {
                let Some(token) = Token::from_key(k) else {
                    warn!("theme", "Theme {name}: ignoring unknown token '{k}'");
                    continue;
                };
                match v.as_str().ok_or_else(|| format!("{v} is not a string")).and_then(ThemeColor::parse) {
                    Ok(c) if allowed(token, c) => {
                        colors.insert(token, c);
                    }
                    Ok(c) => warn!("theme", "Theme {name}: '{c}' isn't allowed for {k}; using the base colour"),
                    Err(e) => warn!("theme", "Theme {name}: {k}: {e}; using the base colour"),
                }
            }
        }
        Ok(ThemeSpec { name: name.to_string(), base, colors })
    }
}

pub fn builtin_names() -> Vec<&'static str> {
    builtin::BUILTINS.iter().map(|b| b.0).collect()
}

pub fn is_builtin(name: &str) -> bool {
    builtin::lookup(name).is_some()
}

/// A built-in theme, fully resolved (`catppuccin` is an alias for mocha).
pub fn builtin(name: &str) -> Option<ThemeSpec> {
    builtin::lookup(name)
}

fn theme_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.json"))
}

/// Names of the `*.json` files in `dir` that are valid custom theme names,
/// sorted. Built-in names are skipped: a built-in always wins on load.
pub fn custom_names_in(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            (p.extension()? == "json").then_some(())?;
            let stem = p.file_stem()?.to_str()?.to_string();
            validate_name(&stem).is_ok().then_some(stem)
        })
        .collect();
    out.sort();
    out
}

pub fn custom_names() -> Vec<String> {
    custom_names_in(&paths::themes_dir())
}

/// Built-ins in table order, then custom themes alphabetically.
pub fn all_names() -> Vec<String> {
    builtin_names().into_iter().map(str::to_string).chain(custom_names()).collect()
}

pub fn exists(name: &str) -> bool {
    is_builtin(name) || theme_path(&paths::themes_dir(), name).is_file()
}

/// A custom theme as stored (overrides only), without resolving it.
pub fn read_custom_in(dir: &Path, name: &str) -> Result<ThemeSpec, String> {
    let path = theme_path(dir, name);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("Couldn't read {}: {e}", path.display()))?;
    ThemeSpec::from_json(name, &text)
}

/// Load any theme by name and resolve every token.
pub fn load_in(dir: &Path, name: &str) -> Result<ThemeSpec, String> {
    if let Some(b) = builtin(name) {
        return Ok(b);
    }
    let spec = read_custom_in(dir, name)?;
    Ok(resolve_in(dir, &spec))
}

pub fn load(name: &str) -> Result<ThemeSpec, String> {
    load_in(&paths::themes_dir(), name)
}

/// Fill every token the spec doesn't set from its base chain, then `ansi`.
pub fn resolve_in(dir: &Path, spec: &ThemeSpec) -> ThemeSpec {
    let mut colors = spec.colors.clone();
    let mut base = spec.base.clone();
    let mut seen = vec![spec.name.clone()];
    // Bounded: a cycle (a → b → a) or a missing base just stops the chain.
    while let Some(b) = base.take() {
        if seen.contains(&b) || seen.len() > 8 {
            warn!("theme", "Theme {}: base chain loops at '{b}'", spec.name);
            break;
        }
        seen.push(b.clone());
        let parent = match builtin(&b) {
            Some(p) => p,
            None => match read_custom_in(dir, &b) {
                Ok(p) => p,
                Err(e) => {
                    warn!("theme", "Theme {}: base '{b}' unavailable ({e}); using ansi", spec.name);
                    break;
                }
            },
        };
        for (t, c) in &parent.colors {
            colors.entry(*t).or_insert(*c);
        }
        base = parent.base.clone();
    }
    if let Some(ansi) = builtin("ansi") {
        for (t, c) in ansi.colors {
            colors.entry(t).or_insert(c);
        }
    }
    debug!("theme", "Resolved {} via {:?}", spec.name, seen);
    ThemeSpec { name: spec.name.clone(), base: spec.base.clone(), colors }
}

pub fn resolve(spec: &ThemeSpec) -> ThemeSpec {
    resolve_in(&paths::themes_dir(), spec)
}

/// Custom theme names: 1–40 of `a-z 0-9 - _`, and not a built-in name.
pub fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 40 {
        return Err("Theme names are 1–40 characters".into());
    }
    if !name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_') {
        return Err("Use only a-z, 0-9, - and _ in theme names".into());
    }
    if is_builtin(name) {
        return Err(format!("'{name}' is a built-in theme; pick another name"));
    }
    Ok(())
}

/// Write a custom theme (temp file + rename).
pub fn save_custom_in(dir: &Path, spec: &ThemeSpec) -> Result<PathBuf, String> {
    validate_name(&spec.name)?;
    std::fs::create_dir_all(dir).map_err(|e| format!("Couldn't create {}: {e}", dir.display()))?;
    let path = theme_path(dir, &spec.name);
    let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    std::fs::write(&tmp, spec.to_json())
        .and_then(|_| std::fs::rename(&tmp, &path))
        .map_err(|e| format!("Couldn't save theme: {e}"))?;
    Ok(path)
}

pub fn save_custom(spec: &ThemeSpec) -> Result<PathBuf, String> {
    save_custom_in(&paths::themes_dir(), spec)
}

pub fn remove_custom_in(dir: &Path, name: &str) -> Result<(), String> {
    if is_builtin(name) {
        return Err(format!("'{name}' is a built-in theme and can't be removed"));
    }
    let path = theme_path(dir, name);
    std::fs::remove_file(&path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => format!("No custom theme named '{name}'"),
        _ => format!("Couldn't remove {}: {e}", path.display()),
    })
}

pub fn remove_custom(name: &str) -> Result<(), String> {
    remove_custom_in(&paths::themes_dir(), name)
}

/// WCAG relative luminance, for contrast checks.
pub fn luminance((r, g, b): (u8, u8, u8)) -> f64 {
    let ch = |c: u8| {
        let c = c as f64 / 255.0;
        if c <= 0.03928 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * ch(r) + 0.7152 * ch(g) + 0.0722 * ch(b)
}

pub fn contrast(a: (u8, u8, u8), b: (u8, u8, u8)) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_parse_and_round_trip() {
        for s in ["#89b4fa", "#000000", "default", "reverse", "darkgray", "lightcyan", "white"] {
            let c = ThemeColor::parse(s).unwrap();
            assert_eq!(c.to_string(), s);
            assert_eq!(ThemeColor::parse(&c.to_string()).unwrap(), c);
        }
        assert_eq!(ThemeColor::parse("#fa0").unwrap(), ThemeColor::Rgb(0xff, 0xaa, 0x00));
        assert_eq!(ThemeColor::parse(" Dark-Grey ").unwrap(), ThemeColor::Ansi(8));
        assert_eq!(ThemeColor::parse("#ABCDEF").unwrap(), ThemeColor::Rgb(0xab, 0xcd, 0xef));
        for bad in ["#12345", "#gggggg", "purple-ish", "", "#"] {
            assert!(ThemeColor::parse(bad).is_err(), "{bad} should be rejected");
        }
    }

    #[test]
    fn every_builtin_resolves_all_tokens_with_unique_names() {
        let names = builtin_names();
        let mut sorted = names.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), names.len(), "duplicate built-in name");
        for n in names {
            let t = builtin(n).unwrap();
            for tok in Token::ALL {
                assert!(t.colors.contains_key(&tok), "{n} is missing {}", tok.key());
                assert!(allowed(tok, t.get(tok)), "{n}: reverse used outside selection");
            }
        }
        assert_eq!(builtin("catppuccin").unwrap().name, "catppuccin-mocha");
    }

    #[test]
    fn themes_with_a_background_are_readable() {
        for n in builtin_names() {
            let t = builtin(n).unwrap();
            let Some(bg) = t.get(Token::Background).rgb() else { continue };
            let text = t.get(Token::Text).rgb().unwrap();
            assert!(contrast(text, bg) >= 4.5, "{n}: text contrast {:.2}", contrast(text, bg));
            let faint = t.get(Token::Faint).rgb().unwrap();
            assert!(contrast(faint, bg) >= 2.5, "{n}: faint contrast {:.2}", contrast(faint, bg));
            if let Some(sel) = t.get(Token::Selection).rgb() {
                assert!(contrast(text, sel) >= 3.0, "{n}: selected text contrast {:.2}", contrast(text, sel));
            }
        }
    }

    #[test]
    fn the_original_themes_keep_their_colours() {
        let ansi = builtin("ansi").unwrap();
        assert_eq!(ansi.get(Token::Text), ThemeColor::Default);
        assert_eq!(ansi.get(Token::Selection), ThemeColor::Reverse);
        assert_eq!(ansi.get(Token::Background), ThemeColor::Default);
        assert_eq!(ansi.get(Token::Teal), ThemeColor::Ansi(6));
        let m = builtin("catppuccin-mocha").unwrap();
        assert_eq!(m.get(Token::Accent), ThemeColor::Rgb(0x89, 0xb4, 0xfa));
        assert_eq!(m.get(Token::Background), ThemeColor::Default, "dark themes keep the terminal background");
    }

    #[test]
    fn custom_theme_round_trips_and_inherits_from_its_base() {
        let dir = tempfile::tempdir().unwrap();
        let mut colors = BTreeMap::new();
        colors.insert(Token::Accent, ThemeColor::Rgb(1, 2, 3));
        colors.insert(Token::Background, ThemeColor::Rgb(0x10, 0x10, 0x10));
        let spec = ThemeSpec { name: "mine".into(), base: Some("gruvbox".into()), colors };
        save_custom_in(dir.path(), &spec).unwrap();

        assert_eq!(read_custom_in(dir.path(), "mine").unwrap(), spec);
        let t = load_in(dir.path(), "mine").unwrap();
        assert_eq!(t.get(Token::Accent), ThemeColor::Rgb(1, 2, 3));
        assert_eq!(t.get(Token::Red), builtin("gruvbox").unwrap().get(Token::Red));
        assert_eq!(custom_names_in(dir.path()), vec!["mine".to_string()]);

        // A theme can build on another custom theme.
        let child = ThemeSpec { name: "child".into(), base: Some("mine".into()), colors: BTreeMap::new() };
        save_custom_in(dir.path(), &child).unwrap();
        assert_eq!(load_in(dir.path(), "child").unwrap().get(Token::Accent), ThemeColor::Rgb(1, 2, 3));

        remove_custom_in(dir.path(), "mine").unwrap();
        assert!(remove_custom_in(dir.path(), "mine").is_err());
        assert!(remove_custom_in(dir.path(), "gruvbox").is_err());
    }

    #[test]
    fn bad_values_fall_back_instead_of_failing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("odd.json"),
            r##"{"base":"tokyonight","colors":{"accent":"nope","red":"reverse","sparkle":"#fff","green":"#00ff00"}}"##,
        )
        .unwrap();
        let t = load_in(dir.path(), "odd").unwrap();
        let tokyo = builtin("tokyonight").unwrap();
        assert_eq!(t.get(Token::Accent), tokyo.get(Token::Accent));
        assert_eq!(t.get(Token::Red), tokyo.get(Token::Red));
        assert_eq!(t.get(Token::Green), ThemeColor::Rgb(0, 0xff, 0));

        // A missing or looping base falls back to ansi.
        std::fs::write(dir.path().join("loop.json"), r#"{"base":"loop","colors":{}}"#).unwrap();
        assert_eq!(load_in(dir.path(), "loop").unwrap().get(Token::Selection), ThemeColor::Reverse);
        std::fs::write(dir.path().join("broken.json"), "{").unwrap();
        assert!(load_in(dir.path(), "broken").is_err());
        assert!(load_in(dir.path(), "missing").is_err());
    }

    #[test]
    fn names_are_validated() {
        assert!(validate_name("my-theme_2").is_ok());
        for bad in ["", "My Theme", "a/b", "dracula", "catppuccin", &"x".repeat(41)] {
            assert!(validate_name(bad).is_err(), "{bad} should be rejected");
        }
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Bad Name.json"), "{}").unwrap();
        std::fs::write(dir.path().join("nord.json"), "{}").unwrap();
        std::fs::write(dir.path().join("notes.txt"), "").unwrap();
        assert!(custom_names_in(dir.path()).is_empty());
    }
}
