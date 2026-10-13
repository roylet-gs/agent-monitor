//! Time helpers. SQLite `datetime('now')` text is UTC without a zone marker.

use time::{OffsetDateTime, PrimitiveDateTime, macros::format_description};

pub fn now_unix() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}

/// Parse `YYYY-MM-DD HH:MM:SS` (SQLite UTC) into unix seconds.
pub fn parse_sqlite(s: &str) -> Option<i64> {
    let fmt = format_description!("[year]-[month]-[day] [hour]:[minute]:[second]");
    PrimitiveDateTime::parse(s.get(..19)?, fmt).ok().map(|t| t.assume_utc().unix_timestamp())
}

/// An ISO-8601 UTC timestamp (`2026-10-13T06:25:26.796Z`) → unix seconds.
pub fn parse_iso(s: &str) -> Option<i64> {
    parse_sqlite(&s.get(..19)?.replacen('T', " ", 1))
}

/// Compact age: `12s`, `4m`, `3h`, `2d`, `5w`.
pub fn short_age(secs: i64) -> String {
    let s = secs.max(0);
    match s {
        0..60 => format!("{s}s"),
        60..3600 => format!("{}m", s / 60),
        3600..86_400 => format!("{}h", s / 3600),
        86_400..1_209_600 => format!("{}d", s / 86_400),
        _ => format!("{}w", s / 604_800),
    }
}

/// Elapsed clock for running work: `42s`, `2m14s`, `1h03m`.
pub fn elapsed(secs: i64) -> String {
    let s = secs.max(0);
    if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}m{:02}s", s / 60, s % 60)
    } else {
        format!("{}h{:02}m", s / 3600, (s % 3600) / 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sqlite_utc() {
        assert_eq!(parse_sqlite("1970-01-01 00:01:00"), Some(60));
        assert_eq!(parse_sqlite("2026-10-11 09:12:57.123"), parse_sqlite("2026-10-11 09:12:57"));
        assert_eq!(parse_sqlite("nope"), None);
        assert_eq!(parse_iso("1970-01-01T00:01:00.123Z"), Some(60));
    }

    #[test]
    fn formats() {
        assert_eq!(short_age(5), "5s");
        assert_eq!(short_age(240), "4m");
        assert_eq!(short_age(3 * 3600), "3h");
        assert_eq!(short_age(2 * 86_400), "2d");
        assert_eq!(short_age(30 * 86_400), "4w");
        assert_eq!(elapsed(134), "2m14s");
        assert_eq!(elapsed(3780), "1h03m");
    }
}
