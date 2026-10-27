//! `am perf`: how long each stage of the app has been taking, from debug.log
//! (`am_core::perf` summaries plus older timing lines), against its budget.

use std::time::Duration;

use am_core::perf::{self, StageReport};
use am_core::paths;
use serde_json::{Value, json};

use crate::output::{self, col, col_right};

/// `30m` / `1h` / `2d` → a duration.
pub fn parse_since(s: &str) -> Option<Duration> {
    let s = s.trim();
    let split = s.find(|c: char| !c.is_ascii_digit())?;
    let n: u64 = s[..split].parse().ok()?;
    let unit = match &s[split..] {
        "s" => 1,
        "m" => 60,
        "h" => 3_600,
        "d" => 86_400,
        _ => return None,
    };
    Some(Duration::from_secs(n * unit))
}

/// The log's timestamp format for `now - ago`, for an ISO prefix compare.
fn cutoff(ago: Duration) -> String {
    let t = time::OffsetDateTime::now_utc() - ago;
    t.format(time::macros::format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z"))
        .unwrap_or_default()
}

/// The stage report for the last `ago` of debug.log (rotated file first).
pub fn report(ago: Option<Duration>) -> Vec<StageReport> {
    let mut text = String::new();
    for path in [paths::rotated_log_path(), paths::log_path()] {
        if let Ok(bytes) = std::fs::read(&path) {
            text.push_str(&String::from_utf8_lossy(&bytes));
            text.push('\n');
        }
    }
    let since = ago.map(cutoff);
    perf::report_from_log(text.lines(), since.as_deref())
}

fn status(r: &StageReport) -> &'static str {
    match (r.budget, r.over_budget()) {
        (None, _) => "-",
        (_, true) => "SLOW",
        _ if r.slow_warnings > 0 => "was slow",
        _ => "ok",
    }
}

pub fn run(since: &str, as_json: bool) {
    let ago = if since == "all" { None } else { Some(parse_since(since).unwrap_or_else(|| crate::fail(&format!("Bad --since {since:?} (try 30m, 1h, 2d or all)")))) };
    let stages = report(ago);
    if as_json {
        let rows: Vec<Value> = stages
            .iter()
            .map(|r| {
                json!({
                    "stage": r.stage,
                    "samples": r.n,
                    "p50Ms": r.p50.as_secs_f64() * 1000.0,
                    "p95Ms": r.p95.as_secs_f64() * 1000.0,
                    "maxMs": r.max.as_secs_f64() * 1000.0,
                    "budgetMs": r.budget.map(|b| b.as_secs_f64() * 1000.0),
                    "slowWarnings": r.slow_warnings,
                    "status": status(r),
                })
            })
            .collect();
        output::json(&Value::Array(rows));
        return;
    }
    if stages.is_empty() {
        println!("No timings in {} for the last {since}. Run am-tui for a few minutes (timings are logged every 5 min).", paths::log_path().display());
        return;
    }
    let rows: Vec<Vec<(&'static str, String)>> = stages
        .iter()
        .map(|r| {
            vec![
                ("stage", r.stage.clone()),
                ("n", r.n.to_string()),
                ("p50", perf::ms(r.p50)),
                ("p95", perf::ms(r.p95)),
                ("max", perf::ms(r.max)),
                ("budget", r.budget.map(perf::ms).unwrap_or_else(|| "-".into())),
                ("status", status(r).into()),
            ]
        })
        .collect();
    output::table(
        &rows,
        &[
            col("stage", "Stage"),
            col_right("n", "Samples"),
            col_right("p50", "p50"),
            col_right("p95", "p95"),
            col_right("max", "Max"),
            col_right("budget", "Budget (p95)"),
            col("status", "Status"),
        ],
    );
    let slow: Vec<&str> = stages.iter().filter(|r| r.over_budget()).map(|r| r.stage.as_str()).collect();
    if !slow.is_empty() {
        println!("\nOver budget: {}", slow.join(", "));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn since_takes_unit_suffixes() {
        assert_eq!(parse_since("30m"), Some(Duration::from_secs(1_800)));
        assert_eq!(parse_since("1h"), Some(Duration::from_secs(3_600)));
        assert_eq!(parse_since("2d"), Some(Duration::from_secs(172_800)));
        assert_eq!(parse_since("1w"), None);
        assert_eq!(parse_since("h"), None);
    }
}
