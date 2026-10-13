//! Today's token usage, summed from the agents' session transcripts.
//!
//! Every billed token class counts (input, cache creation, cache read,
//! output), subagent transcripts included. Which files to read and how to
//! parse a line is up to each agent plugin ([`Harness::usage_files`] /
//! [`Harness::usage_record`]); this module only adds them up.
//!
//! Reading is incremental: a file is read once, then only the bytes appended
//! since, so a refresh costs milliseconds even with tens of MB of transcripts.
//! Records are de-duplicated by message id (a message's usage repeats on each
//! of its content-block lines, and a resumed session may copy history). The
//! day is the local calendar day; nothing is persisted, totals are rebuilt
//! from the transcripts when the process starts.
//!
//! [`Harness::usage_files`]: crate::harness::Harness::usage_files
//! [`Harness::usage_record`]: crate::harness::Harness::usage_record

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::harness::{HarnessKind, is_within};
use crate::{debug, info};

/// One API response's usage, as parsed from a transcript line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageRecord {
    /// Message id: the same response may appear on several lines.
    pub id: String,
    /// Unix seconds.
    pub at: i64,
    /// Working directory of the session at that point, if recorded.
    pub cwd: Option<String>,
    /// Every billed token class added up.
    pub tokens: u64,
}

/// Where usage comes from: an agent plugin, or a fixture in tests.
pub trait UsageSource {
    /// Transcript files modified at or after `since`.
    fn files(&self, since: SystemTime) -> Vec<PathBuf>;
    /// Parse one transcript line; `None` for lines that carry no usage.
    fn record(&self, line: &str) -> Option<UsageRecord>;
}

impl UsageSource for HarnessKind {
    fn files(&self, since: SystemTime) -> Vec<PathBuf> {
        self.provider().usage_files(since)
    }
    fn record(&self, line: &str) -> Option<UsageRecord> {
        self.provider().usage_record(line)
    }
}

/// A local calendar day.
pub type Day = (i32, u32, u32);

#[derive(Debug, Default)]
struct FileState {
    offset: u64,
    ino: u64,
    /// Fallback for records without a cwd.
    last_cwd: Option<String>,
}

/// Incremental tracker of today's usage.
#[derive(Debug, Default)]
pub struct DailyUsage {
    day: Option<Day>,
    files: HashMap<PathBuf, FileState>,
    seen: HashSet<String>,
    /// Today's tokens per cwd ("" when unknown).
    by_cwd: HashMap<String, u64>,
}

impl DailyUsage {
    pub fn new() -> DailyUsage {
        DailyUsage::default()
    }

    /// Read what was appended since the last refresh.
    pub fn refresh<S: UsageSource>(&mut self, sources: &[S]) {
        self.refresh_at(crate::timefmt::now_unix(), sources);
    }

    pub fn refresh_at<S: UsageSource>(&mut self, now: i64, sources: &[S]) {
        let started = Instant::now();
        let today = local_day(now);
        if self.day != Some(today) {
            if self.day.is_some() {
                info!("usage", "New day {today:?}: resetting token usage");
            }
            *self = DailyUsage { day: Some(today), ..DailyUsage::default() };
        }
        let since = UNIX_EPOCH + Duration::from_secs(local_midnight(now).max(0) as u64);
        let (mut files, mut bytes) = (0usize, 0u64);
        for source in sources {
            for path in source.files(since) {
                files += 1;
                bytes += self.read_file(path, source, today);
            }
        }
        debug!(
            "usage",
            "Read {bytes} B from {files} transcript(s) in {:?}; today {} tokens",
            started.elapsed(),
            self.total()
        );
    }

    /// Returns the number of bytes read.
    fn read_file<S: UsageSource>(&mut self, path: PathBuf, source: &S, today: Day) -> u64 {
        let Ok(mut f) = File::open(&path) else { return 0 };
        let Ok(meta) = f.metadata() else { return 0 };
        let state = self.files.entry(path).or_default();
        if state.ino != meta.ino() || meta.len() < state.offset {
            // New, replaced or truncated: read from the start (ids dedupe).
            *state = FileState { ino: meta.ino(), ..FileState::default() };
        }
        if meta.len() == state.offset {
            return 0;
        }
        let mut buf = Vec::new();
        if f.seek(SeekFrom::Start(state.offset)).is_err() || f.read_to_end(&mut buf).is_err() {
            return 0;
        }
        // Leave a partial trailing line for next time.
        let Some(end) = buf.iter().rposition(|&b| b == b'\n') else { return 0 };
        let chunk = &buf[..=end];
        state.offset += chunk.len() as u64;
        for line in String::from_utf8_lossy(chunk).lines() {
            let Some(r) = source.record(line) else { continue };
            if r.cwd.is_some() {
                state.last_cwd.clone_from(&r.cwd);
            }
            if local_day(r.at) != today || !self.seen.insert(r.id) {
                continue;
            }
            let cwd = r.cwd.or_else(|| state.last_cwd.clone()).unwrap_or_default();
            *self.by_cwd.entry(cwd).or_default() += r.tokens;
        }
        chunk.len() as u64
    }

    /// Everything used today.
    pub fn total(&self) -> u64 {
        self.by_cwd.values().sum()
    }

    /// Today's usage attributed to `roots`: each cwd goes to the most specific
    /// root containing it (a nested worktree, not its parent checkout). Usage
    /// outside every root is left out.
    pub fn by_root(&self, roots: &[&str]) -> HashMap<String, u64> {
        let mut out: HashMap<String, u64> = HashMap::new();
        for (cwd, &tokens) in &self.by_cwd {
            if let Some(root) = roots.iter().filter(|r| is_within(cwd, r)).max_by_key(|r| r.len()) {
                *out.entry((*root).to_owned()).or_default() += tokens;
            }
        }
        out
    }
}

fn local_tm(at: i64) -> libc::tm {
    let t = at as libc::time_t;
    // SAFETY: localtime_r only writes the caller-owned `tm`.
    unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        tm
    }
}

/// The local calendar day of a unix timestamp.
pub fn local_day(at: i64) -> Day {
    let tm = local_tm(at);
    (tm.tm_year + 1900, (tm.tm_mon + 1) as u32, tm.tm_mday as u32)
}

/// Unix seconds of local midnight on the day of `at`.
pub fn local_midnight(at: i64) -> i64 {
    let mut tm = local_tm(at);
    tm.tm_hour = 0;
    tm.tm_min = 0;
    tm.tm_sec = 0;
    tm.tm_isdst = -1;
    // SAFETY: mktime normalises the caller-owned `tm`.
    let t = unsafe { libc::mktime(&mut tm) };
    if t == -1 { at - at.rem_euclid(86_400) } else { t as i64 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Lines are `id tokens at cwd`, `cwd` optional.
    struct Fixture(Vec<PathBuf>);

    impl UsageSource for Fixture {
        fn files(&self, _since: SystemTime) -> Vec<PathBuf> {
            self.0.clone()
        }
        fn record(&self, line: &str) -> Option<UsageRecord> {
            let mut p = line.split(' ');
            Some(UsageRecord {
                id: p.next()?.into(),
                tokens: p.next()?.parse().ok()?,
                at: p.next()?.parse().ok()?,
                cwd: p.next().map(Into::into),
            })
        }
    }

    fn append(path: &std::path::Path, s: &str) {
        std::fs::OpenOptions::new().create(true).append(true).open(path).unwrap().write_all(s.as_bytes()).unwrap();
    }

    fn now() -> i64 {
        // Midday, so "now - 2 days" and "now + 1 h" never cross today's edges.
        local_midnight(crate::timefmt::now_unix()) + 12 * 3600
    }

    #[test]
    fn sums_today_dedupes_and_reads_incrementally() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a.jsonl"), dir.path().join("b.jsonl"));
        let n = now();
        append(&a, &format!("m1 100 {n} /w\nm1 100 {n} /w\nm2 50 {} /w\nnot usage\n", n - 2 * 86_400));
        append(&b, &format!("m1 100 {n} /w\nm3 7 {n}\n"));
        let src = [Fixture(vec![a.clone(), b.clone()])];
        let mut u = DailyUsage::new();
        u.refresh_at(n, &src);
        // m1 once, m2 is from two days ago; m3 has no cwd ever seen in b → ""…
        // except b saw /w on its m1 line first, so m3 falls back to /w.
        assert_eq!(u.total(), 107);
        assert_eq!(u.by_cwd.get("/w"), Some(&107));

        // A partial trailing line waits until it is complete.
        append(&a, &format!("m4 5 {n} /w"));
        u.refresh_at(n, &src);
        assert_eq!(u.total(), 107);
        append(&a, "\n");
        u.refresh_at(n, &src);
        assert_eq!(u.total(), 112);
    }

    #[test]
    fn truncation_rereads_and_day_rollover_resets() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.jsonl");
        let n = now();
        append(&a, &format!("m1 10 {n} /w\nm2 10 {n} /w\n"));
        let src = [Fixture(vec![a.clone()])];
        let mut u = DailyUsage::new();
        u.refresh_at(n, &src);
        assert_eq!(u.total(), 20);
        std::fs::write(&a, format!("m3 1 {n} /w\n")).unwrap();
        u.refresh_at(n, &src);
        assert_eq!(u.total(), 21);

        let tomorrow = n + 86_400;
        append(&a, &format!("m4 4 {tomorrow} /w\n"));
        u.refresh_at(tomorrow, &src);
        assert_eq!(u.total(), 4);
    }

    #[test]
    fn attributes_to_the_most_specific_root() {
        let mut u = DailyUsage::new();
        u.by_cwd = HashMap::from([
            ("/work".into(), 1),
            ("/work/src".into(), 2),
            ("/work/.claude/worktrees/x".into(), 4),
            ("/work2".into(), 8),
            ("".into(), 16),
        ]);
        let roots = u.by_root(&["/work", "/work/.claude/worktrees/x"]);
        assert_eq!(roots.get("/work"), Some(&3));
        assert_eq!(roots.get("/work/.claude/worktrees/x"), Some(&4));
        assert_eq!(roots.len(), 2);
        assert_eq!(u.total(), 31);
    }

    #[test]
    fn local_midnight_is_on_the_same_day() {
        let n = crate::timefmt::now_unix();
        let m = local_midnight(n);
        assert!(m <= n && n - m < 25 * 3600);
        assert_eq!(local_day(m), local_day(n));
        assert_ne!(local_day(m - 1), local_day(n));
    }
}
