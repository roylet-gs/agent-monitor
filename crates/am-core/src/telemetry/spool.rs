//! Where events wait to be sent: `<app dir>/telemetry/spool.jsonl`, one
//! [`Event`] per line. Every process appends with one `O_APPEND` write (as the
//! log does), so the hook and CLI never touch the network. The uploader
//! renames the spool to `batch-<unix ms>-<pid>.jsonl` before reading it, so a
//! writer racing it lands in the batch or in a fresh spool, never nowhere.
//! A full spool ([`MAX_SPOOL_BYTES`]) drops new events; batches older than
//! [`MAX_AGE_MS`] are deleted unsent.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use super::schema::Event;

pub const MAX_SPOOL_BYTES: u64 = 1024 * 1024;
pub const MAX_AGE_MS: u64 = 7 * 24 * 60 * 60 * 1000;
const SPOOL: &str = "spool.jsonl";

pub fn spool_path(dir: &Path) -> PathBuf {
    dir.join(SPOOL)
}

/// Append one event; quietly does nothing when the spool is full or unwritable.
pub fn append(dir: &Path, event: &Event) {
    let path = spool_path(dir);
    if fs::metadata(&path).is_ok_and(|m| m.len() >= MAX_SPOOL_BYTES) {
        return;
    }
    let Ok(mut line) = serde_json::to_string(event) else { return };
    line.push('\n');
    let _ = fs::create_dir_all(dir);
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = f.write_all(line.as_bytes());
    }
}

/// Move the spool aside as a new batch (if it has anything) and return every
/// batch to send, oldest first, deleting those past [`MAX_AGE_MS`].
pub fn take_batches(dir: &Path, now_ms: u64) -> Vec<PathBuf> {
    let spool = spool_path(dir);
    if fs::metadata(&spool).is_ok_and(|m| m.len() > 0) {
        let _ = fs::rename(&spool, dir.join(format!("batch-{now_ms}-{}.jsonl", std::process::id())));
    }
    let mut batches: Vec<(u64, PathBuf)> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let ms = batch_ms(&path)?;
            Some((ms, path))
        })
        .collect();
    batches.sort();
    batches
        .into_iter()
        .filter_map(|(ms, path)| {
            if now_ms.saturating_sub(ms) > MAX_AGE_MS {
                let _ = fs::remove_file(&path);
                None
            } else {
                Some(path)
            }
        })
        .collect()
}

fn batch_ms(path: &Path) -> Option<u64> {
    let name = path.file_name()?.to_str()?;
    let rest = name.strip_prefix("batch-")?.strip_suffix(".jsonl")?;
    rest.split('-').next()?.parse().ok()
}

/// The valid events in a spool or batch file; anything else is skipped.
pub fn read(path: &Path) -> Vec<Event> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str::<Event>(l).ok())
        .filter(Event::valid)
        .collect()
}

/// Everything waiting to be sent (batches, then the spool), without moving it.
pub fn pending(dir: &Path) -> Vec<Event> {
    let mut batches: Vec<(u64, PathBuf)> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| Some((batch_ms(&e.path())?, e.path())))
        .collect();
    batches.sort();
    batches.iter().flat_map(|(_, p)| read(p)).chain(read(&spool_path(dir))).collect()
}

/// Anything waiting to be sent (one `stat` when the directory doesn't exist).
pub fn has_pending(dir: &Path) -> bool {
    dir.exists() && (spool_path(dir).exists() || fs::read_dir(dir).into_iter().flatten().flatten().any(|e| batch_ms(&e.path()).is_some()))
}

/// Delete everything waiting (telemetry turned off).
pub fn clear(dir: &Path) {
    let _ = fs::remove_file(spool_path(dir));
    for e in fs::read_dir(dir).into_iter().flatten().flatten() {
        if batch_ms(&e.path()).is_some() {
            let _ = fs::remove_file(e.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(n: u64) -> Event {
        Event::command(n, "repo.list", "cli")
    }

    #[test]
    fn appended_events_move_into_a_batch_and_read_back() {
        let dir = tempfile::tempdir().unwrap();
        append(dir.path(), &ev(1));
        append(dir.path(), &ev(2));
        assert_eq!(pending(dir.path()), vec![ev(1), ev(2)]);
        let batches = take_batches(dir.path(), 1_000);
        assert_eq!(batches.len(), 1);
        assert!(!spool_path(dir.path()).exists());
        // a writer after the take starts a new spool
        append(dir.path(), &ev(3));
        assert_eq!(read(&batches[0]), vec![ev(1), ev(2)]);
        assert_eq!(pending(dir.path()), vec![ev(1), ev(2), ev(3)]);
        assert_eq!(take_batches(dir.path(), 2_000).len(), 2);
    }

    #[test]
    fn bad_lines_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        append(dir.path(), &ev(1));
        fs::OpenOptions::new().append(true).open(spool_path(dir.path())).unwrap().write_all(b"not json\n{\"ts\":1}\n").unwrap();
        let mut tampered = ev(2);
        tampered.attrs.insert("command".into(), super::super::schema::Attr::Str("/etc".into()));
        append(dir.path(), &tampered);
        assert_eq!(pending(dir.path()), vec![ev(1)]);
    }

    #[test]
    fn old_batches_are_dropped_and_a_full_spool_takes_no_more() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("batch-5-1.jsonl"), "").unwrap();
        assert!(take_batches(dir.path(), 5 + MAX_AGE_MS + 1).is_empty());
        assert!(!dir.path().join("batch-5-1.jsonl").exists());

        fs::write(spool_path(dir.path()), vec![b'x'; MAX_SPOOL_BYTES as usize]).unwrap();
        append(dir.path(), &ev(1));
        assert_eq!(fs::metadata(spool_path(dir.path())).unwrap().len(), MAX_SPOOL_BYTES);
    }

    #[test]
    fn clear_removes_spool_and_batches() {
        let dir = tempfile::tempdir().unwrap();
        append(dir.path(), &ev(1));
        take_batches(dir.path(), 1);
        append(dir.path(), &ev(2));
        assert!(has_pending(dir.path()));
        clear(dir.path());
        assert!(pending(dir.path()).is_empty());
        assert!(!has_pending(dir.path()));
        assert!(!has_pending(&dir.path().join("missing")));
    }
}
