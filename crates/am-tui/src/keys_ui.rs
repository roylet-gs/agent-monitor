//! Settings → Key bindings: pick a command, press its new key. A key that
//! another command holds is swapped over; fixed keys are refused. Every
//! change is saved at once (`KeyOutcome::Save`). Pure state + key handling.

use std::collections::BTreeMap;

use crate::app::Key;
use crate::keymap::{self, Command, Keymap};
use crate::nav;

pub enum KeyOutcome {
    Stay,
    Close,
    /// New `keyBindings` value (only the changed keys).
    Save(BTreeMap<String, String>),
}

pub struct KeyEditor {
    pub cursor: usize,
    /// Waiting for the selected command's new key.
    pub capturing: bool,
    pub keymap: Keymap,
    /// (ok, text) under the list.
    pub message: Option<(bool, String)>,
    /// First visible row (`ui::scroll_start`).
    pub scroll: std::cell::Cell<usize>,
}

impl KeyEditor {
    pub fn new(keymap: Keymap) -> KeyEditor {
        KeyEditor { cursor: 0, capturing: false, keymap, message: None, scroll: Default::default() }
    }

    pub fn selected(&self) -> Command {
        Command::ALL[self.cursor]
    }

    pub fn on_key(&mut self, key: Key) -> KeyOutcome {
        if self.capturing {
            self.capturing = false;
            return self.capture(key);
        }
        self.message = None;
        let cmd = self.selected();
        match key {
            Key::Esc | Key::Char('q') => return KeyOutcome::Close,
            k if let Some(to) = nav::step(k, true, self.cursor, Command::ALL.len()) => self.cursor = to,
            Key::Enter | Key::Char(' ') => {
                self.capturing = true;
                self.message = Some((true, format!("Press the new key for {} · esc cancels", cmd.label())));
            }
            Key::Char('x') | Key::Backspace => return self.bind(cmd, cmd.default_key(), true),
            Key::Char('R') => {
                if self.keymap == Keymap::default() {
                    return KeyOutcome::Stay;
                }
                self.keymap = Keymap::default();
                self.message = Some((true, "Every key is back to its default".into()));
                am_core::info!("keys", "Key bindings reset to defaults via settings");
                return KeyOutcome::Save(self.keymap.overrides());
            }
            _ => {}
        }
        KeyOutcome::Stay
    }

    fn capture(&mut self, key: Key) -> KeyOutcome {
        let cmd = self.selected();
        if key == Key::Esc {
            self.message = None;
            return KeyOutcome::Stay;
        }
        if let Some(what) = keymap::reserved(key) {
            self.message = Some((false, format!("{} is fixed ({}) — pick another key", keymap::display(key), what.to_lowercase())));
            return KeyOutcome::Stay;
        }
        if keymap::key_name(key).is_none() {
            self.message = Some((false, format!("{} can't be bound", keymap::display(key))));
            return KeyOutcome::Stay;
        }
        self.bind(cmd, Some(key), false)
    }

    /// Give `cmd` `key`; whoever held it takes `cmd`'s old key.
    fn bind(&mut self, cmd: Command, key: Option<Key>, reset: bool) -> KeyOutcome {
        let old = self.keymap.key(cmd);
        if old == key {
            if reset {
                self.message = Some((true, format!("{} is already on its default", cmd.label())));
            }
            return KeyOutcome::Stay;
        }
        let holder = key.and_then(|k| self.keymap.holder(k)).filter(|h| *h != cmd);
        self.keymap.set(cmd, key);
        let shown = key.map(keymap::display).unwrap_or_else(|| "—".into());
        let text = match holder {
            Some(other) => {
                self.keymap.set(other, old);
                format!("{} on {shown} · {} moved to {}", cmd.label(), other.label(), self.keymap.shown(other))
            }
            None => format!("{} on {shown}", cmd.label()),
        };
        am_core::info!("keys", "Bound {} to {shown}{}", cmd.id(), holder.map(|h| format!(" (swapped with {})", h.id())).unwrap_or_default());
        self.message = Some((true, text));
        KeyOutcome::Save(self.keymap.overrides())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor_on(cmd: Command) -> KeyEditor {
        let mut ed = KeyEditor::new(Keymap::default());
        ed.cursor = Command::ALL.iter().position(|c| *c == cmd).unwrap();
        ed
    }

    #[test]
    fn capture_binds_the_next_key() {
        let mut ed = editor_on(Command::NewWorktree);
        assert!(matches!(ed.on_key(Key::Enter), KeyOutcome::Stay));
        assert!(ed.capturing);
        let KeyOutcome::Save(b) = ed.on_key(Key::Char('N')) else { panic!("expected save") };
        assert_eq!(b.get("newWorktree").map(String::as_str), Some("N"));
        assert_eq!(b.len(), 1);
        assert!(!ed.capturing);
    }

    #[test]
    fn a_taken_key_is_swapped() {
        let mut ed = editor_on(Command::NewWorktree);
        ed.on_key(Key::Enter);
        let KeyOutcome::Save(b) = ed.on_key(Key::Char('r')) else { panic!() };
        assert_eq!(ed.keymap.command(Key::Char('r')), Some(Command::NewWorktree));
        assert_eq!(ed.keymap.command(Key::Char('n')), Some(Command::Refresh));
        assert!(ed.message.as_ref().is_some_and(|(ok, m)| *ok && m.contains("Refresh now moved to n")));
        // What is saved loads back to the same map.
        assert_eq!(Keymap::from_bindings(&b), (ed.keymap.clone(), vec![]));
    }

    #[test]
    fn fixed_keys_are_refused_and_esc_cancels() {
        let mut ed = editor_on(Command::Help);
        ed.on_key(Key::Enter);
        assert!(matches!(ed.on_key(Key::Up), KeyOutcome::Stay));
        assert!(ed.message.as_ref().is_some_and(|(ok, m)| !ok && m.contains("fixed")));
        assert_eq!(ed.keymap, Keymap::default());
        ed.on_key(Key::Enter);
        assert!(matches!(ed.on_key(Key::Esc), KeyOutcome::Stay));
        assert!(!ed.capturing);
        assert_eq!(ed.keymap, Keymap::default());
    }

    #[test]
    fn x_resets_one_and_capital_r_resets_all() {
        let mut ed = editor_on(Command::NewWorktree);
        ed.on_key(Key::Enter);
        ed.on_key(Key::Char('N'));
        ed.on_key(Key::Char('j'));
        ed.on_key(Key::Enter);
        ed.on_key(Key::Char('D'));
        ed.on_key(Key::Char('k'));
        let KeyOutcome::Save(b) = ed.on_key(Key::Char('x')) else { panic!() };
        assert_eq!(b.keys().collect::<Vec<_>>(), ["deleteWorktree"]);
        let KeyOutcome::Save(b) = ed.on_key(Key::Char('R')) else { panic!() };
        assert!(b.is_empty());
        assert!(matches!(ed.on_key(Key::Char('R')), KeyOutcome::Stay));
    }
}
