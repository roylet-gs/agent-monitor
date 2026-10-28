//! Cursor movement shared by every list, so Settings and its sub-pages move
//! like the worktree list: ↑↓ one row, PgUp/PgDn ten, Home/End to the ends,
//! plus the dashboard's default letters (`j`/`k`, `ctrl-d`/`ctrl-u`, `G`).
//! The dashboard itself goes through the keymap (so rebinding applies there);
//! overlays keep these keys. Pure.

use crate::app::Key;

/// Rows a page key moves.
pub const PAGE: isize = 10;

/// How far `key` moves a list cursor, if it is a movement key. `letters` is
/// false for lists that take typed text, which then keep only the non-letter keys.
pub fn delta(key: Key, letters: bool) -> Option<isize> {
    Some(match key {
        Key::Down => 1,
        Key::Up => -1,
        Key::PageDown | Key::CtrlD => PAGE,
        Key::PageUp | Key::CtrlU => -PAGE,
        Key::Home => isize::MIN / 2,
        Key::End => isize::MAX / 2,
        Key::Char('j') if letters => 1,
        Key::Char('k') if letters => -1,
        Key::Char('G') if letters => isize::MAX / 2,
        _ => return None,
    })
}

/// `cursor` moved by `delta`, kept on a list of `len` rows.
pub fn moved(cursor: usize, delta: isize, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    (cursor as isize).saturating_add(delta).clamp(0, len as isize - 1) as usize
}

/// `moved` for the movement key `key`; `None` when it isn't one.
pub fn step(key: Key, letters: bool, cursor: usize, len: usize) -> Option<usize> {
    delta(key, letters).map(|d| moved(cursor, d, len))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_move_like_the_worktree_list() {
        assert_eq!(step(Key::Down, true, 0, 30), Some(1));
        assert_eq!(step(Key::Char('j'), true, 0, 30), Some(1));
        assert_eq!(step(Key::Char('k'), true, 5, 30), Some(4));
        assert_eq!(step(Key::PageDown, true, 5, 30), Some(15));
        assert_eq!(step(Key::CtrlU, true, 5, 30), Some(0));
        assert_eq!(step(Key::End, true, 5, 30), Some(29));
        assert_eq!(step(Key::Char('G'), true, 5, 30), Some(29));
        assert_eq!(step(Key::Home, true, 5, 30), Some(0));
        assert_eq!(step(Key::Char('x'), true, 5, 30), None);
    }

    #[test]
    fn typed_lists_keep_only_the_non_letter_keys() {
        assert_eq!(step(Key::Char('j'), false, 0, 30), None);
        assert_eq!(step(Key::Char('G'), false, 0, 30), None);
        assert_eq!(step(Key::End, false, 0, 30), Some(29));
    }

    #[test]
    fn an_empty_list_stays_at_zero() {
        assert_eq!(moved(3, 1, 0), 0);
        assert_eq!(moved(0, -1, 4), 0);
    }
}
