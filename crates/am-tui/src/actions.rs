//! The key bar's actions, shared by every page. A page lists its hints
//! once; each command lands in the layer its bound key needs (an upper-case
//! letter under Shift, a `ctrl-` key under Ctrl, the rest plain), so a
//! rebound key moves with it. Holding Shift or Ctrl swaps the bar to that
//! layer where the terminal reports held modifiers (kitty keyboard
//! protocol); elsewhere the modified hints join the plain bar, last to
//! fit. Pure: `ui.rs` fits and draws the result.

use std::borrow::Cow;

use crate::app::{Held, Key};
use crate::keymap::{Command, Keymap, display};

/// What a click on a key-bar hint does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarHit {
    /// Press this key (review page and jobs-pane hints, `⇥`).
    Key(Key),
    /// Run this dashboard command.
    Command(Command),
}

/// The modifier a key needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    Plain,
    Shift,
    Ctrl,
}

impl Layer {
    pub fn of(key: Key) -> Layer {
        match key {
            Key::Char(c) if c.is_uppercase() => Layer::Shift,
            Key::CtrlC | Key::CtrlD | Key::CtrlU | Key::CtrlS | Key::CtrlV | Key::CtrlEnter => Layer::Ctrl,
            _ => Layer::Plain,
        }
    }
}

/// One key hint. Lower `prio` survives narrower widths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
    pub key: String,
    pub label: Cow<'static, str>,
    pub hit: Option<BarHit>,
    pub prio: u8,
}

/// A key-bar entry: a hint or a `│` separator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    Hint(Hint),
    Sep,
}

/// A page's actions by layer, plus the `tail` (help, quit/back) every
/// layer ends with.
#[derive(Debug, Default)]
pub struct ActionBar {
    plain: Vec<Entry>,
    shift: Vec<Entry>,
    ctrl: Vec<Entry>,
    tail: Vec<Entry>,
    /// Hints press their key (`BarHit::Key`) rather than run the command:
    /// the review page gives commands its own meaning.
    as_keys: bool,
}

impl ActionBar {
    pub fn new() -> ActionBar {
        ActionBar::default()
    }

    /// Hints that press their key instead of running the command.
    pub fn pressing_keys() -> ActionBar {
        ActionBar { as_keys: true, ..ActionBar::default() }
    }

    fn layer(&mut self, layer: Layer) -> &mut Vec<Entry> {
        match layer {
            Layer::Plain => &mut self.plain,
            Layer::Shift => &mut self.shift,
            Layer::Ctrl => &mut self.ctrl,
        }
    }

    /// `c` under its bound key, in that key's layer; an unbound command is
    /// left out.
    pub fn command(&mut self, keymap: &Keymap, c: Command, label: impl Into<Cow<'static, str>>, prio: u8) {
        let Some(k) = keymap.key(c) else { return };
        let hit = if self.as_keys { BarHit::Key(k) } else { BarHit::Command(c) };
        self.layer(Layer::of(k)).push(Entry::Hint(Hint { key: display(k), label: label.into(), hit: Some(hit), prio }));
    }

    /// A fixed key (`⏎`, `⇥`, `esc`) in its layer.
    pub fn key(&mut self, key: Key, label: impl Into<Cow<'static, str>>, prio: u8) {
        let hint = Hint { key: display(key), label: label.into(), hit: Some(BarHit::Key(key)), prio };
        self.layer(Layer::of(key)).push(Entry::Hint(hint));
    }

    /// A plain hint that isn't clickable (`j/k select job`).
    pub fn note(&mut self, key: &str, label: &'static str, prio: u8) {
        self.plain.push(Entry::Hint(Hint { key: key.into(), label: label.into(), hit: None, prio }));
    }

    /// A `│` between groups (of the plain layer).
    pub fn sep(&mut self) {
        self.plain.push(Entry::Sep);
    }

    /// `c` (or a fixed key) at the end of every layer.
    pub fn tail_command(&mut self, keymap: &Keymap, c: Command, label: &'static str, prio: u8) {
        let mut one = ActionBar { as_keys: self.as_keys, ..ActionBar::default() };
        one.command(keymap, c, label, prio);
        self.tail.extend(one.plain.into_iter().chain(one.shift).chain(one.ctrl));
    }

    pub fn tail_key(&mut self, key: Key, label: impl Into<Cow<'static, str>>, prio: u8) {
        self.tail.push(Entry::Hint(Hint { key: display(key), label: label.into(), hit: Some(BarHit::Key(key)), prio }));
    }

    /// What the bar shows with `held` down: that modifier's layer, else the
    /// plain one with a `⇧/ctrl more` pointer to the others. Where held
    /// modifiers aren't reported the other layers join the plain one (by
    /// their own priority, so give them a low one), except `ctrl-⏎`, which
    /// arrives as `⏎` there.
    pub fn visible(self, held: Held, reported: bool) -> Vec<Entry> {
        let ActionBar { plain, shift, ctrl, tail, .. } = self;
        let mut out = if held.ctrl && !ctrl.is_empty() {
            ctrl
        } else if held.shift && !shift.is_empty() {
            shift
        } else if reported {
            let more = match (shift.is_empty(), ctrl.is_empty()) {
                (false, false) => Some("⇧/ctrl"),
                (false, true) => Some("⇧"),
                (true, false) => Some("ctrl"),
                (true, true) => None,
            };
            let mut out = plain;
            out.push(Entry::Sep);
            out.extend(tail);
            out.extend(more.map(|m| Entry::Hint(Hint { key: m.into(), label: "more".into(), hit: None, prio: 3 })));
            return tidy(out);
        } else {
            let mut out = plain;
            out.push(Entry::Sep);
            out.extend(shift.into_iter().chain(ctrl).filter_map(|e| match e {
                Entry::Hint(h) if h.hit != Some(BarHit::Key(Key::CtrlEnter)) => Some(Entry::Hint(h)),
                _ => None,
            }));
            out
        };
        out.push(Entry::Sep);
        out.extend(tail);
        tidy(out)
    }
}

/// No `│` at either end or twice in a row.
fn tidy(entries: Vec<Entry>) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::with_capacity(entries.len());
    for e in entries {
        if e == Entry::Sep && out.last().is_none_or(|l| *l == Entry::Sep) {
            continue;
        }
        out.push(e);
    }
    if out.last() == Some(&Entry::Sep) {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(entries: &[Entry]) -> String {
        entries
            .iter()
            .map(|e| match e {
                Entry::Hint(h) => format!("{} {}", h.key, h.label),
                Entry::Sep => "│".into(),
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn bar(keymap: &Keymap) -> ActionBar {
        let mut b = ActionBar::new();
        b.command(keymap, Command::NewWorktree, "new", 1);
        b.command(keymap, Command::AddRepo, "add repo", 1);
        b.sep();
        b.command(keymap, Command::ReviewWindow, "reviews in new window", 1);
        b.key(Key::CtrlEnter, "pull + run script", 0);
        b.tail_command(keymap, Command::Quit, "quit", 0);
        b
    }

    #[test]
    fn a_command_goes_to_the_layer_its_key_needs() {
        assert_eq!(Layer::of(Key::Char('A')), Layer::Shift);
        assert_eq!(Layer::of(Key::CtrlV), Layer::Ctrl);
        assert_eq!(Layer::of(Key::Char('n')), Layer::Plain);
        assert_eq!(Layer::of(Key::Enter), Layer::Plain);
        let map = Keymap::default();
        let held = |shift, ctrl| Held { shift, ctrl };
        assert_eq!(keys(&bar(&map).visible(held(false, false), true)), "n new, │, q quit, ⇧/ctrl more");
        assert_eq!(keys(&bar(&map).visible(held(true, false), true)), "A add repo, │, q quit");
        assert_eq!(keys(&bar(&map).visible(held(false, true), true)), "ctrl-v reviews in new window, ctrl-⏎ pull + run script, │, q quit");
    }

    #[test]
    fn a_rebound_key_moves_its_hint() {
        let mut map = Keymap::default();
        map.set(Command::NewWorktree, Some(Key::Char('N')));
        let shift = bar(&map).visible(Held { shift: true, ctrl: false }, true);
        assert_eq!(keys(&shift), "N new, A add repo, │, q quit");
    }

    #[test]
    fn without_held_keys_the_layers_join_the_plain_bar() {
        let entries = bar(&Keymap::default()).visible(Held::default(), false);
        // ctrl-⏎ can't be told from ⏎ there, so it's left out.
        assert_eq!(keys(&entries), "n new, │, A add repo, ctrl-v reviews in new window, │, q quit");
    }
}
