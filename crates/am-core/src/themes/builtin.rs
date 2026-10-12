//! Built-in themes. Columns follow [`Token::ALL`]:
//! text, faint, accent, border, borderFocus, selection, background,
//! green, yellow, red, blue, magenta, teal.
//! An empty background keeps the terminal's own (dark themes); light and
//! retro themes paint theirs so they read the same on any terminal.

use std::collections::BTreeMap;

use super::{ThemeColor, ThemeSpec, Token};

type Row = (&'static str, [&'static str; 13]);

#[rustfmt::skip]
pub(super) const BUILTINS: &[Row] = &[
    // Follows the terminal's 16-colour palette; selection is reverse video so
    // a blue background can't collide with blue text.
    ("ansi",                 ["default", "darkgray", "cyan", "darkgray", "green", "reverse", "default", "green", "yellow", "red", "blue", "magenta", "cyan"]),
    // --- Catppuccin ---
    ("catppuccin-mocha",     ["#cdd6f4", "#7f849c", "#89b4fa", "#585b70", "#b4befe", "#313244", "", "#a6e3a1", "#f9e2af", "#f38ba8", "#89b4fa", "#cba6f7", "#94e2d5"]),
    ("catppuccin-macchiato", ["#cad3f5", "#8087a2", "#8aadf4", "#5b6078", "#b7bdf8", "#363a4f", "", "#a6da95", "#eed49f", "#ed8796", "#8aadf4", "#c6a0f6", "#8bd5ca"]),
    ("catppuccin-frappe",    ["#c6d0f5", "#838ba7", "#8caaee", "#626880", "#babbf1", "#414559", "", "#a6d189", "#e5c890", "#e78284", "#8caaee", "#ca9ee6", "#81c8be"]),
    ("catppuccin-latte",     ["#4c4f69", "#8c8fa1", "#1e66f5", "#acb0be", "#7287fd", "#ccd0da", "#eff1f5", "#40a02b", "#df8e1d", "#d20f39", "#1e66f5", "#8839ef", "#179299"]),
    // --- Dark ---
    ("tokyonight",           ["#c0caf5", "#565f89", "#7aa2f7", "#3b4261", "#7aa2f7", "#283457", "", "#9ece6a", "#e0af68", "#f7768e", "#7aa2f7", "#bb9af7", "#73daca"]),
    ("tokyonight-storm",     ["#c0caf5", "#737aa2", "#7aa2f7", "#3b4261", "#7aa2f7", "#2e3c64", "#24283b", "#9ece6a", "#e0af68", "#f7768e", "#7aa2f7", "#bb9af7", "#73daca"]),
    ("gruvbox",              ["#ebdbb2", "#928374", "#83a598", "#504945", "#b8bb26", "#3c3836", "", "#b8bb26", "#fabd2f", "#fb4934", "#83a598", "#d3869b", "#8ec07c"]),
    ("dracula",              ["#f8f8f2", "#6272a4", "#bd93f9", "#44475a", "#ff79c6", "#44475a", "", "#50fa7b", "#f1fa8c", "#ff5555", "#bd93f9", "#ff79c6", "#8be9fd"]),
    ("nord",                 ["#d8dee9", "#616e88", "#88c0d0", "#434c5e", "#88c0d0", "#3b4252", "", "#a3be8c", "#ebcb8b", "#bf616a", "#81a1c1", "#b48ead", "#8fbcbb"]),
    ("one-dark",             ["#abb2bf", "#5c6370", "#61afef", "#4b5263", "#61afef", "#3e4451", "", "#98c379", "#e5c07b", "#e06c75", "#61afef", "#c678dd", "#56b6c2"]),
    ("solarized-dark",       ["#93a1a1", "#657b83", "#2aa198", "#586e75", "#268bd2", "#073642", "", "#859900", "#b58900", "#dc322f", "#268bd2", "#d33682", "#2aa198"]),
    ("rose-pine",            ["#e0def4", "#6e6a86", "#c4a7e7", "#524f67", "#ebbcba", "#403d52", "", "#9ccfd8", "#f6c177", "#eb6f92", "#c4a7e7", "#ebbcba", "#3e8fb0"]),
    ("kanagawa",             ["#dcd7ba", "#727169", "#7e9cd8", "#54546d", "#7fb4ca", "#2d4f67", "", "#98bb6c", "#e6c384", "#e46876", "#7e9cd8", "#957fb8", "#7aa89f"]),
    ("everforest",           ["#d3c6aa", "#859289", "#7fbbb3", "#4f585e", "#a7c080", "#3d484d", "", "#a7c080", "#dbbc7f", "#e67e80", "#7fbbb3", "#d699b6", "#83c092"]),
    ("github-dark",          ["#e6edf3", "#7d8590", "#58a6ff", "#30363d", "#58a6ff", "#1c2b41", "", "#3fb950", "#d29922", "#f85149", "#58a6ff", "#bc8cff", "#39c5cf"]),
    ("monokai",              ["#f8f8f2", "#75715e", "#66d9ef", "#49483e", "#a6e22e", "#49483e", "", "#a6e22e", "#e6db74", "#f92672", "#66d9ef", "#ae81ff", "#78dce8"]),
    ("nightfox",             ["#cdcecf", "#738091", "#719cd6", "#39506d", "#86abdc", "#2b3b51", "", "#81b29a", "#dbc074", "#c94f6d", "#719cd6", "#9d79d6", "#63cdcf"]),
    // --- Light ---
    ("solarized-light",      ["#586e75", "#839496", "#268bd2", "#93a1a1", "#268bd2", "#eee8d5", "#fdf6e3", "#859900", "#b58900", "#dc322f", "#268bd2", "#d33682", "#2aa198"]),
    ("github-light",         ["#1f2328", "#59636e", "#0969da", "#d0d7de", "#0969da", "#ddf4ff", "#ffffff", "#1a7f37", "#9a6700", "#cf222e", "#0969da", "#8250df", "#1b7c83"]),
    ("gruvbox-light",        ["#3c3836", "#7c6f64", "#076678", "#bdae93", "#79740e", "#ebdbb2", "#fbf1c7", "#79740e", "#b57614", "#9d0006", "#076678", "#8f3f71", "#427b58"]),
    // --- Retro ---
    // MS-DOS EDIT / Norton Commander: CGA colours on blue.
    ("dos",                  ["#aaaaaa", "#5555ff", "#ffff55", "#aaaaaa", "#ffffff", "#000000", "#0000aa", "#55ff55", "#ffff55", "#ff5555", "#55ffff", "#ff55ff", "#00aaaa"]),
    // Borland Turbo Pascal / Turbo C IDE: yellow text on blue, cyan frames.
    ("turbo-pascal",         ["#ffff55", "#aaaaaa", "#55ffff", "#55ffff", "#ffffff", "#5555ff", "#0000aa", "#55ff55", "#ffff55", "#ff5555", "#55ffff", "#ff55ff", "#00aaaa"]),
    // IBM 3270 terminal: green fields and the seven-colour 3279 palette on black.
    ("ibm-3270",             ["#40ff40", "#2e8b2e", "#58f0f0", "#2e8b2e", "#ffffff", "#1a3a1a", "#000000", "#40ff40", "#ffff00", "#f01818", "#7890f0", "#f078f0", "#58f0f0"]),
    // IBM 5151 monochrome: one hue, status by brightness.
    ("green-phosphor",       ["#33ff33", "#1a8f1a", "#66ff66", "#1a7a1a", "#99ff99", "#0f3d0f", "#050a05", "#33ff33", "#ccffcc", "#e8ffe8", "#22cc22", "#66ff66", "#22aa22"]),
    ("amber-phosphor",       ["#ffb000", "#a06e00", "#ffcc33", "#805800", "#ffd966", "#3d2a00", "#0a0700", "#ffb000", "#ffe080", "#fff0d0", "#d99500", "#ffcc33", "#c08400"]),
];

pub(super) fn lookup(name: &str) -> Option<ThemeSpec> {
    let name = if name == "catppuccin" { "catppuccin-mocha" } else { name };
    let (n, cols) = BUILTINS.iter().find(|(n, _)| *n == name)?;
    let colors: BTreeMap<Token, ThemeColor> = Token::ALL
        .into_iter()
        .zip(cols.iter())
        .map(|(t, s)| (t, if s.is_empty() { ThemeColor::Default } else { ThemeColor::parse(s).unwrap_or(ThemeColor::Default) }))
        .collect();
    Some(ThemeSpec { name: n.to_string(), base: None, colors })
}
