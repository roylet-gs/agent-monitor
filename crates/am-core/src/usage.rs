//! Today's token usage, summed from the agents' session transcripts.
//!
//! Every billed token class is kept apart (input, cache writes, cache reads,
//! output), subagent transcripts included, along with an estimated cost when
//! the harness knows the model's price. The dashboard shows them by the
//! `dailyTokensMode` setting ([`Tally::weighted`] for the input-equivalent
//! view). Which files to read and how to parse a line is up to each harness
//! plugin ([`Harness::usage_files`] / [`Harness::usage_record`]); this module
//! only adds them up.
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

/// Token counts per billed class.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TokenCounts {
    pub input: u64,
    /// Cache writes with the 5-minute TTL (1.25× the input price).
    pub cache_write_5m: u64,
    /// Cache writes with the 1-hour TTL (2× the input price).
    pub cache_write_1h: u64,
    /// Cache reads (~0.1× the input price): every call re-reads the whole
    /// cached conversation, so these dominate the raw total.
    pub cache_read: u64,
    pub output: u64,
}

impl TokenCounts {
    pub fn cache_write(&self) -> u64 {
        self.cache_write_5m + self.cache_write_1h
    }

    /// Every billed token, unweighted.
    pub fn total(&self) -> u64 {
        self.input + self.cache_write() + self.cache_read + self.output
    }

    /// Input-equivalent tokens: each class weighted by its usual price
    /// relative to input (cache read 0.1×, 5-minute write 1.25×, 1-hour write
    /// 2×, output 5×). Model-agnostic, so it tracks the bill's shape without a
    /// price table.
    pub fn weighted(&self) -> u64 {
        let w = self.input as f64
            + self.cache_write_5m as f64 * 1.25
            + self.cache_write_1h as f64 * 2.0
            + self.cache_read as f64 * 0.1
            + self.output as f64 * 5.0;
        w.round() as u64
    }

    fn add(&mut self, o: &TokenCounts) {
        self.input += o.input;
        self.cache_write_5m += o.cache_write_5m;
        self.cache_write_1h += o.cache_write_1h;
        self.cache_read += o.cache_read;
        self.output += o.output;
    }
}

/// Usage added up over many responses.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Tally {
    pub tokens: TokenCounts,
    /// Estimated cost in US dollars at API list prices, for the responses
    /// whose model has a known price.
    pub cost_usd: f64,
    /// Responses without a known price (left out of `cost_usd`).
    pub unpriced: u64,
}

impl Tally {
    fn add_record(&mut self, r: &UsageRecord) {
        self.tokens.add(&r.tokens);
        match r.cost_usd {
            Some(c) => self.cost_usd += c,
            None => self.unpriced += 1,
        }
    }

    pub fn add(&mut self, o: &Tally) {
        self.tokens.add(&o.tokens);
        self.cost_usd += o.cost_usd;
        self.unpriced += o.unpriced;
    }
}

/// One API response's usage, as parsed from a transcript line.
#[derive(Debug, Clone, PartialEq)]
pub struct UsageRecord {
    /// Message id: the same response may appear on several lines.
    pub id: String,
    /// Unix seconds.
    pub at: i64,
    /// Working directory of the session at that point, if recorded.
    pub cwd: Option<String>,
    pub tokens: TokenCounts,
    /// Estimated cost in US dollars; `None` when the model's price is unknown.
    pub cost_usd: Option<f64>,
}

/// Where usage comes from: an harness plugin, or a fixture in tests.
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
    /// Today's usage per cwd ("" when unknown).
    by_cwd: HashMap<String, Tally>,
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
            "Read {bytes} B from {files} transcript(s) in {:?}; today {:?}",
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
            if local_day(r.at) != today || !self.seen.insert(r.id.clone()) {
                continue;
            }
            let cwd = r.cwd.clone().or_else(|| state.last_cwd.clone()).unwrap_or_default();
            self.by_cwd.entry(cwd).or_default().add_record(&r);
        }
        chunk.len() as u64
    }

    /// Everything used today.
    pub fn total(&self) -> Tally {
        let mut t = Tally::default();
        self.by_cwd.values().for_each(|v| t.add(v));
        t
    }

    /// Today's usage attributed to `roots`: each cwd goes to the most specific
    /// root containing it (a nested worktree, not its parent checkout). Usage
    /// outside every root is left out.
    pub fn by_root(&self, roots: &[&str]) -> HashMap<String, Tally> {
        let mut out: HashMap<String, Tally> = HashMap::new();
        for (cwd, tally) in &self.by_cwd {
            if let Some(root) = roots.iter().filter(|r| is_within(cwd, r)).max_by_key(|r| r.len()) {
                out.entry((*root).to_owned()).or_default().add(tally);
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

    /// Lines are `id tokens at cwd`, `cwd` optional; `tokens` are input
    /// tokens costing $1 each.
    struct Fixture(Vec<PathBuf>);

    impl UsageSource for Fixture {
        fn files(&self, _since: SystemTime) -> Vec<PathBuf> {
            self.0.clone()
        }
        fn record(&self, line: &str) -> Option<UsageRecord> {
            let mut p = line.split(' ');
            let id = p.next()?.into();
            let input: u64 = p.next()?.parse().ok()?;
            Some(UsageRecord {
                id,
                tokens: TokenCounts { input, ..TokenCounts::default() },
                cost_usd: Some(input as f64),
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
        assert_eq!(u.total().tokens.input, 107);
        assert_eq!(u.by_cwd["/w"].tokens.input, 107);
        assert_eq!(u.total().cost_usd, 107.0);

        // A partial trailing line waits until it is complete.
        append(&a, &format!("m4 5 {n} /w"));
        u.refresh_at(n, &src);
        assert_eq!(u.total().tokens.input, 107);
        append(&a, "\n");
        u.refresh_at(n, &src);
        assert_eq!(u.total().tokens.input, 112);
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
        assert_eq!(u.total().tokens.input, 20);
        std::fs::write(&a, format!("m3 1 {n} /w\n")).unwrap();
        u.refresh_at(n, &src);
        assert_eq!(u.total().tokens.input, 21);

        let tomorrow = n + 86_400;
        append(&a, &format!("m4 4 {tomorrow} /w\n"));
        u.refresh_at(tomorrow, &src);
        assert_eq!(u.total().tokens.input, 4);
    }

    #[test]
    fn attributes_to_the_most_specific_root() {
        let mut u = DailyUsage::new();
        let t = |output| Tally { tokens: TokenCounts { output, ..TokenCounts::default() }, ..Tally::default() };
        u.by_cwd = HashMap::from([
            ("/work".into(), t(1)),
            ("/work/src".into(), t(2)),
            ("/work/.claude/worktrees/x".into(), t(4)),
            ("/work2".into(), t(8)),
            ("".into(), t(16)),
        ]);
        let roots = u.by_root(&["/work", "/work/.claude/worktrees/x"]);
        assert_eq!(roots["/work"].tokens.output, 3);
        assert_eq!(roots["/work/.claude/worktrees/x"].tokens.output, 4);
        assert_eq!(roots.len(), 2);
        assert_eq!(u.total().tokens.output, 31);
    }

    #[test]
    fn weighted_tokens_follow_the_price_ratios() {
        let c = TokenCounts { input: 100, cache_write_5m: 400, cache_write_1h: 50, cache_read: 10_000, output: 20 };
        assert_eq!(c.total(), 10_570);
        assert_eq!(c.cache_write(), 450);
        // 100 + 500 + 100 + 1000 + 100
        assert_eq!(c.weighted(), 1_800);
    }

    #[test]
    fn unpriced_responses_are_counted_apart() {
        let mut t = Tally::default();
        let r = |cost_usd| UsageRecord { id: "m".into(), at: 0, cwd: None, tokens: TokenCounts { input: 1, ..TokenCounts::default() }, cost_usd };
        t.add_record(&r(Some(0.5)));
        t.add_record(&r(None));
        assert_eq!((t.tokens.input, t.cost_usd, t.unpriced), (2, 0.5, 1));
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
