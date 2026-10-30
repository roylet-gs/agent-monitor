//! The TUI's part of the anonymous usage reports (`am_core::telemetry`): the
//! daily `am.heartbeat` (the setup in broad strokes), `am.features` (how
//! often each dashboard command ran), and the `usage-reports` thread that
//! sends the spool. Everything here is counts, booleans and fixed ids.

use std::collections::BTreeMap;
use std::time::Duration;

use am_core::settings::Settings;
use am_core::telemetry::schema::Event;
use am_core::telemetry::{public, upload};
use am_core::{debug, info};

use crate::app::Snapshot;

/// How often the TUI records `am.features` and sends what's waiting.
pub const EVERY: Duration = Duration::from_secs(15 * 60);
/// The first send after start, so a quick look at am doesn't send at once.
const FIRST: Duration = Duration::from_secs(60);
/// Longest wait after failed sends (doubling from [`EVERY`]).
const MAX_BACKOFF: Duration = Duration::from_secs(6 * 60 * 60);

/// UTC day, `2026-10-30`.
pub fn today() -> String {
    let d = time::OffsetDateTime::now_utc().date();
    format!("{}-{:02}-{:02}", d.year(), u8::from(d.month()), d.day())
}

/// Once a day: what the install looks like, without anything that names it.
pub fn heartbeat(settings: &Settings, snapshot: &Snapshot, inside_cmux: bool, terminal: Option<&str>, ts: u64) -> Event {
    let ide = if am_core::launch::IdeChoice::ALL.iter().any(|(id, _)| *id == settings.ide) { settings.ide.as_str() } else { "other" };
    let mut sessions: BTreeMap<&str, u64> = BTreeMap::new();
    for w in snapshot.groups.iter().flat_map(|g| &g.worktrees) {
        for s in &w.sessions {
            // Manifest plugins are named by their user: counted as `custom`.
            *sessions.entry(if s.agent.is_builtin() { s.agent.id() } else { "custom" }).or_default() += 1;
        }
    }
    let mut e = Event::heartbeat(ts)
        .with_str("mode", settings.app_mode().id())
        .with_str("ide", ide)
        .with_int("repos", snapshot.repos.len() as u64)
        .with_int("worktrees", snapshot.worktree_paths.len() as u64)
        .with_bool("github", settings.gh_pr_status)
        .with_bool("linear", settings.linear_enabled)
        .with_bool("reviews", settings.review_requests_on())
        .with_int("review_groups", settings.review_groups.len() as u64)
        .with_int("key_bindings_changed", settings.key_bindings.len() as u64)
        .with_bool("compact", settings.compact_view)
        .with_bool("cmux", inside_cmux)
        .with_bool("builtin_theme", am_core::themes::is_builtin(&settings.theme));
    for (harness, n) in sessions {
        e = e.with_int(&format!("sessions.{harness}"), n);
    }
    // `with_str` drops anything that isn't a plain token.
    match terminal {
        Some(t) => e.with_str("terminal", t),
        None => e,
    }
}

/// One `am.features` event per command counted so far (the counts emptied).
pub fn features(counts: &mut BTreeMap<&'static str, u64>, ts: u64) -> Vec<Event> {
    std::mem::take(counts).into_iter().map(|(id, n)| Event::feature(ts, id, n)).collect()
}

/// Send the spool every [`EVERY`] while reports are on and a relay is set,
/// backing off after failures. Runs until the process exits.
pub fn spawn_uploader() {
    let spawned = std::thread::Builder::new().name("usage-reports".into()).spawn(|| {
        let mut wait = FIRST;
        loop {
            std::thread::sleep(wait);
            wait = EVERY;
            let (true, Some(endpoint)) = (public::is_on(), upload::endpoint()) else { continue };
            match upload::flush(&endpoint) {
                Ok(_) => {}
                Err(e) => {
                    wait = (wait * 2).min(MAX_BACKOFF);
                    debug!("telemetry", "Usage reports not sent ({e}); next try in {wait:?}");
                }
            }
        }
    });
    if let Err(e) = spawned {
        info!("telemetry", "Couldn't start the usage-reports thread: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use am_core::telemetry::schema::Attr;

    #[test]
    fn heartbeat_holds_counts_and_ids_only() {
        let settings = Settings::parse(r#"{"ide":"custom","ideCommand":"/Users/me/bin/edit --x","theme":"my-theme","ghPrStatus":true}"#);
        let e = heartbeat(&settings, &Snapshot::default(), false, Some("iTerm.app"), 1);
        assert!(e.valid());
        assert_eq!(e.attrs["ide"], Attr::Str("custom".into()));
        assert_eq!(e.attrs["builtin_theme"], Attr::Bool(false));
        assert_eq!(e.attrs["terminal"], Attr::Str("iTerm.app".into()));
        let text = serde_json::to_string(&e).unwrap();
        assert!(!text.contains("/Users") && !text.contains("my-theme"), "{text}");
        // A terminal name that isn't a token is left out.
        let odd = heartbeat(&settings, &Snapshot::default(), false, Some("My Terminal/1.0"), 1);
        assert!(!odd.attrs.contains_key("terminal"));
    }

    #[test]
    fn features_drains_the_counts() {
        let mut counts = BTreeMap::from([("newWorktree", 2), ("refresh", 1)]);
        let events = features(&mut counts, 1);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].attrs["command"], Attr::Str("newWorktree".into()));
        assert_eq!(events[0].attrs["count"], Attr::Int(2));
        assert!(counts.is_empty());
        assert!(features(&mut counts, 2).is_empty());
    }
}
