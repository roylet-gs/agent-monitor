//! Dashboard key bindings. Each `Command` has one rebindable key (default:
//! the 1.x key) plus fixed keys that can't be taken (arrows, PgUp/PgDn,
//! Home/End, Esc, Ctrl-C, Tab), so the dashboard can always be driven and
//! left. `keyBindings` in settings.json stores only the changed keys:
//! command id → key name (`"N"`, `"ctrl-d"`, `"enter"`, `"none"`). Pure.

use std::collections::BTreeMap;

use am_core::settings::Settings;

use crate::app::Key;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Command {
    Open,
    OpenPr,
    OpenLinear,
    OpenProject,
    NewWorktree,
    Delete,
    AddRepo,
    ToggleLog,
    Settings,
    Feedback,
    Refresh,
    Help,
    Quit,
    MoveDown,
    MoveUp,
    PageDown,
    PageUp,
    Top,
    Bottom,
}

impl Command {
    /// Every command, in the order the Key bindings page lists them.
    pub const ALL: [Command; 19] = [
        Command::Open,
        Command::OpenPr,
        Command::OpenLinear,
        Command::OpenProject,
        Command::NewWorktree,
        Command::Delete,
        Command::AddRepo,
        Command::ToggleLog,
        Command::Settings,
        Command::Feedback,
        Command::Refresh,
        Command::Help,
        Command::Quit,
        Command::MoveDown,
        Command::MoveUp,
        Command::PageDown,
        Command::PageUp,
        Command::Top,
        Command::Bottom,
    ];

    fn index(self) -> usize {
        Command::ALL.iter().position(|c| *c == self).unwrap_or(0)
    }

    /// The `keyBindings` key.
    pub fn id(self) -> &'static str {
        match self {
            Command::Open => "open",
            Command::OpenPr => "openPr",
            Command::OpenLinear => "openLinear",
            Command::OpenProject => "openProject",
            Command::NewWorktree => "newWorktree",
            Command::Delete => "deleteWorktree",
            Command::AddRepo => "addRepo",
            Command::ToggleLog => "toggleLog",
            Command::Settings => "settings",
            Command::Feedback => "feedback",
            Command::Refresh => "refresh",
            Command::Help => "help",
            Command::Quit => "quit",
            Command::MoveDown => "moveDown",
            Command::MoveUp => "moveUp",
            Command::PageDown => "pageDown",
            Command::PageUp => "pageUp",
            Command::Top => "top",
            Command::Bottom => "bottom",
        }
    }

    pub fn from_id(id: &str) -> Option<Command> {
        Command::ALL.into_iter().find(|c| c.id() == id)
    }

    /// Name on the Key bindings page.
    pub fn label(self) -> &'static str {
        match self {
            Command::Open => "Open worktree",
            Command::OpenPr => "Open PR",
            Command::OpenLinear => "Open Linear ticket",
            Command::OpenProject => "Open Linear project",
            Command::NewWorktree => "New worktree",
            Command::Delete => "Delete worktree",
            Command::AddRepo => "Add repository",
            Command::ToggleLog => "Toggle log pane",
            Command::Settings => "Settings",
            Command::Feedback => "Suggest a feature",
            Command::Refresh => "Refresh now",
            Command::Help => "Help",
            Command::Quit => "Quit",
            Command::MoveDown => "Move down",
            Command::MoveUp => "Move up",
            Command::PageDown => "Page down",
            Command::PageUp => "Page up",
            Command::Top => "Jump to top",
            Command::Bottom => "Jump to bottom",
        }
    }

    /// One-line description under the Key bindings page.
    pub fn help(self) -> &'static str {
        match self {
            Command::Open => "Open in the IDE (resumes the last agent session); double-click does the same",
            Command::OpenPr => "The branch's PR in the browser (o also works while unbound)",
            Command::OpenLinear => "The branch's Linear ticket",
            Command::OpenProject => "The ticket's Linear project",
            Command::NewWorktree => "New worktree form",
            Command::Delete => "Delete the selected worktree (asks first)",
            Command::AddRepo => "Track another repository",
            Command::ToggleLog => "Show or hide the debug.log pane",
            Command::Settings => "Open this editor",
            Command::Feedback => "File a feature request on GitHub",
            Command::Refresh => "Refresh git, PRs and tickets now",
            Command::Help => "The ? overlay",
            Command::Quit => "Leave am (esc and ctrl-c always quit too)",
            Command::MoveDown => "Next worktree (↓ always works)",
            Command::MoveUp => "Previous worktree (↑ always works)",
            Command::PageDown => "Ten rows down (pgdn always works)",
            Command::PageUp => "Ten rows up (pgup always works)",
            Command::Top => "First worktree (home always works)",
            Command::Bottom => "Last worktree (end always works)",
        }
    }

    /// Key-bar text; `None` for commands that aren't on the bar.
    pub fn bar_label(self) -> Option<&'static str> {
        Some(match self {
            Command::Open => "open",
            Command::OpenPr => "PR",
            Command::OpenLinear => "linear",
            Command::OpenProject => "project",
            Command::NewWorktree => "new",
            Command::Delete => "delete",
            Command::AddRepo => "add repo",
            Command::ToggleLog => "log",
            Command::Settings => "settings",
            Command::Feedback => "feedback",
            Command::Refresh => "refresh",
            Command::Help => "help",
            Command::Quit => "quit",
            _ => return None,
        })
    }

    pub fn default_key(self) -> Option<Key> {
        Some(match self {
            Command::Open => Key::Enter,
            Command::OpenPr => Key::Char('g'),
            Command::OpenLinear => Key::Char('l'),
            Command::OpenProject => Key::Char('p'),
            Command::NewWorktree => Key::Char('n'),
            Command::Delete => Key::Char('d'),
            Command::AddRepo => Key::Char('A'),
            Command::ToggleLog => Key::Char('w'),
            Command::Settings => Key::Char('s'),
            Command::Feedback => Key::Char('f'),
            Command::Refresh => Key::Char('r'),
            Command::Help => Key::Char('?'),
            Command::Quit => Key::Char('q'),
            Command::MoveDown => Key::Char('j'),
            Command::MoveUp => Key::Char('k'),
            Command::PageDown => Key::CtrlD,
            Command::PageUp => Key::CtrlU,
            Command::Top => return None,
            Command::Bottom => Key::Char('G'),
        })
    }

    /// Keys that always run this command and can't be rebound.
    pub fn fixed_keys(self) -> &'static [Key] {
        match self {
            Command::Quit => &[Key::Esc, Key::CtrlC],
            Command::MoveDown => &[Key::Down],
            Command::MoveUp => &[Key::Up],
            Command::PageDown => &[Key::PageDown],
            Command::PageUp => &[Key::PageUp],
            Command::Top => &[Key::Home],
            Command::Bottom => &[Key::End],
            _ => &[],
        }
    }

    /// Acts on the selected worktree (refused while it is being deleted).
    pub fn is_row_action(self) -> bool {
        matches!(self, Command::Open | Command::OpenPr | Command::OpenLinear | Command::OpenProject | Command::Delete)
    }
}

/// `o` opened the PR in 1.x too; it keeps doing so unless it is bound.
const PR_ALIAS: Key = Key::Char('o');

/// Keys no command can take: the fixed keys, plus Tab/Shift-Tab (jobs pane).
pub fn reserved(key: Key) -> Option<&'static str> {
    if matches!(key, Key::Tab | Key::BackTab) {
        return Some("moves between the list and the jobs pane");
    }
    Command::ALL.into_iter().find(|c| c.fixed_keys().contains(&key)).map(|c| c.label())
}

/// `keyBindings` value → key: one character, or a name (`space`, `enter`,
/// `backspace`, `left`, `right`, `ctrl-d`, `ctrl-u`, `ctrl-s`).
pub fn parse_key(s: &str) -> Option<Key> {
    let mut chars = s.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        return (!c.is_control()).then_some(Key::Char(c));
    }
    Some(match s.to_ascii_lowercase().as_str() {
        "space" => Key::Char(' '),
        "enter" | "return" => Key::Enter,
        "backspace" => Key::Backspace,
        "left" => Key::Left,
        "right" => Key::Right,
        "ctrl-d" => Key::CtrlD,
        "ctrl-u" => Key::CtrlU,
        "ctrl-s" => Key::CtrlS,
        _ => return None,
    })
}

/// Key → `keyBindings` value; `None` for keys that can't be stored.
pub fn key_name(key: Key) -> Option<String> {
    Some(match key {
        Key::Char(' ') => "space".into(),
        Key::Char(c) => c.to_string(),
        Key::Enter => "enter".into(),
        Key::Backspace => "backspace".into(),
        Key::Left => "left".into(),
        Key::Right => "right".into(),
        Key::CtrlD => "ctrl-d".into(),
        Key::CtrlU => "ctrl-u".into(),
        Key::CtrlS => "ctrl-s".into(),
        _ => return None,
    })
}

/// How a key is shown on the key bar, in `?` and on the Key bindings page.
pub fn display(key: Key) -> String {
    match key {
        Key::Char(' ') => "space".into(),
        Key::Char(c) => c.to_string(),
        Key::Enter => "⏎".into(),
        Key::Up => "↑".into(),
        Key::Down => "↓".into(),
        Key::Left => "←".into(),
        Key::Right => "→".into(),
        Key::PageUp => "pgup".into(),
        Key::PageDown => "pgdn".into(),
        Key::Home => "home".into(),
        Key::End => "end".into(),
        Key::Backspace => "backspace".into(),
        Key::Tab => "⇥".into(),
        Key::BackTab => "shift-⇥".into(),
        Key::Esc => "esc".into(),
        Key::CtrlC => "ctrl-c".into(),
        Key::CtrlD => "ctrl-d".into(),
        Key::CtrlU => "ctrl-u".into(),
        Key::CtrlS => "ctrl-s".into(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keymap {
    keys: [Option<Key>; Command::ALL.len()],
}

impl Default for Keymap {
    fn default() -> Keymap {
        Keymap { keys: Command::ALL.map(Command::default_key) }
    }
}

impl Keymap {
    /// The settings' bindings; problems are logged and skipped.
    pub fn from_settings(settings: &Settings) -> Keymap {
        let (map, problems) = Keymap::from_bindings(&settings.key_bindings);
        for p in problems {
            am_core::warn!("keys", "keyBindings: {p}");
        }
        map
    }

    /// Defaults with `bindings` on top. An unknown command, an unknown or
    /// reserved key is skipped. A changed key wins over a default one (that
    /// command is left unbound); of two changed commands on one key, the
    /// later one is unbound. Each skip is reported.
    pub fn from_bindings(bindings: &BTreeMap<String, String>) -> (Keymap, Vec<String>) {
        let mut map = Keymap::default();
        let mut problems = Vec::new();
        let mut changed = [false; Command::ALL.len()];
        for (id, value) in bindings {
            let Some(cmd) = Command::from_id(id) else {
                problems.push(format!("unknown command {id:?}"));
                continue;
            };
            let key = if value.eq_ignore_ascii_case("none") {
                None
            } else {
                match parse_key(value) {
                    Some(k) if reserved(k).is_some() => {
                        problems.push(format!("{id}: {value:?} is a fixed key"));
                        continue;
                    }
                    Some(k) => Some(k),
                    None => {
                        problems.push(format!("{id}: unknown key {value:?}"));
                        continue;
                    }
                }
            };
            map.keys[cmd.index()] = key;
            changed[cmd.index()] = true;
        }
        // Defaults give way to changed keys, then earlier changes to later ones.
        for i in 0..map.keys.len() {
            let Some(k) = map.keys[i] else { continue };
            for j in 0..map.keys.len() {
                if i == j || map.keys[j] != Some(k) {
                    continue;
                }
                let loser = match (changed[i], changed[j]) {
                    (true, false) => j,
                    (false, true) => i,
                    _ => i.max(j),
                };
                problems.push(format!("{} and {} both use {:?}; {} is unbound", Command::ALL[i].id(), Command::ALL[j].id(), display(k), Command::ALL[loser].id()));
                map.keys[loser] = None;
                if loser == i {
                    break;
                }
            }
        }
        (map, problems)
    }

    pub fn key(&self, cmd: Command) -> Option<Key> {
        self.keys[cmd.index()]
    }

    /// The command a key press runs: a bound key, else a fixed key, else `o`.
    pub fn command(&self, key: Key) -> Option<Command> {
        if let Some(c) = self.holder(key) {
            return Some(c);
        }
        if let Some(c) = Command::ALL.into_iter().find(|c| c.fixed_keys().contains(&key)) {
            return Some(c);
        }
        (key == PR_ALIAS).then_some(Command::OpenPr)
    }

    /// The command currently bound to `key` (fixed keys aside).
    pub fn holder(&self, key: Key) -> Option<Command> {
        Command::ALL.into_iter().find(|c| self.key(*c) == Some(key))
    }

    pub fn set(&mut self, cmd: Command, key: Option<Key>) {
        self.keys[cmd.index()] = key;
    }

    /// Shown key of `cmd`, `—` when unbound.
    pub fn shown(&self, cmd: Command) -> String {
        self.key(cmd).map(display).unwrap_or_else(|| "—".into())
    }

    /// What `keyBindings` should hold: every key that differs from its default.
    pub fn overrides(&self) -> BTreeMap<String, String> {
        Command::ALL
            .into_iter()
            .filter(|c| self.key(*c) != c.default_key())
            .map(|c| (c.id().to_string(), self.key(c).and_then(key_name).unwrap_or_else(|| "none".into())))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bindings(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn defaults_are_the_1x_keys() {
        let m = Keymap::default();
        assert_eq!(m.command(Key::Char('n')), Some(Command::NewWorktree));
        assert_eq!(m.command(Key::Enter), Some(Command::Open));
        assert_eq!(m.command(Key::Char('o')), Some(Command::OpenPr));
        assert_eq!(m.command(Key::Down), Some(Command::MoveDown));
        assert_eq!(m.command(Key::CtrlC), Some(Command::Quit));
        assert_eq!(m.command(Key::Home), Some(Command::Top));
        assert!(m.overrides().is_empty());
        // Every default is storable and not reserved.
        for c in Command::ALL {
            if let Some(k) = c.default_key() {
                assert!(reserved(k).is_none(), "{c:?}");
                assert_eq!(key_name(k).as_deref().and_then(parse_key), Some(k));
            }
        }
    }

    #[test]
    fn overrides_apply_and_round_trip() {
        let b = bindings(&[("newWorktree", "N"), ("moveDown", "ctrl-s"), ("feedback", "none"), ("top", "space")]);
        let (m, problems) = Keymap::from_bindings(&b);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(m.command(Key::Char('N')), Some(Command::NewWorktree));
        assert_eq!(m.command(Key::Char('n')), None);
        assert_eq!(m.command(Key::CtrlS), Some(Command::MoveDown));
        assert_eq!(m.command(Key::Down), Some(Command::MoveDown), "fixed keys stay");
        assert_eq!(m.key(Command::Feedback), None);
        assert_eq!(m.command(Key::Char(' ')), Some(Command::Top));
        assert_eq!(m.overrides(), b);
    }

    #[test]
    fn bad_entries_are_skipped() {
        let b = bindings(&[("nope", "x"), ("refresh", "up"), ("help", "ctrl-x"), ("quit", "tab")]);
        let (m, problems) = Keymap::from_bindings(&b);
        assert_eq!(problems.len(), 4, "{problems:?}");
        assert_eq!(m, Keymap::default());
    }

    #[test]
    fn a_changed_key_wins_a_clash() {
        // newWorktree takes r: refresh (still on its default) is unbound.
        let (m, problems) = Keymap::from_bindings(&bindings(&[("newWorktree", "r")]));
        assert_eq!(problems.len(), 1);
        assert_eq!(m.command(Key::Char('r')), Some(Command::NewWorktree));
        assert_eq!(m.key(Command::Refresh), None);
        // A swap written by the editor resolves cleanly.
        let (m, problems) = Keymap::from_bindings(&bindings(&[("newWorktree", "r"), ("refresh", "n")]));
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(m.command(Key::Char('n')), Some(Command::Refresh));
        // Two changed commands on one key: the later one loses.
        let (m, _) = Keymap::from_bindings(&bindings(&[("newWorktree", "x"), ("refresh", "x")]));
        assert_eq!(m.command(Key::Char('x')), Some(Command::NewWorktree));
        assert_eq!(m.key(Command::Refresh), None);
    }

    #[test]
    fn binding_o_replaces_the_pr_alias() {
        let (m, _) = Keymap::from_bindings(&bindings(&[("toggleLog", "o")]));
        assert_eq!(m.command(Key::Char('o')), Some(Command::ToggleLog));
    }
}
