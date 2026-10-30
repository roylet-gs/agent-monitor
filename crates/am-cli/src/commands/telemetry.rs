//! `am telemetry`: the anonymous usage and error reports (`TELEMETRY.md`).
//! `status`, `on`, `off`, `show` (every event waiting to be sent, exactly as
//! it will be sent), `reset-id` and `flush`.

use am_core::settings::{self, TelemetryChoice};
use am_core::telemetry::{self, Consent, public, schema, spool, upload};
use serde_json::{Value, json};

use crate::output;

const ABOUT: &str = "am sends anonymous usage and error reports only if you turn them on: the version, OS, \
how it was installed, which commands and features are used, how long its stages take, and where in am's \
own code an error or crash happened. Never log text, paths, repository, branch, PR or user names. \
See TELEMETRY.md.";

fn consent_text(c: Consent) -> String {
    match c {
        Consent::On => "on".into(),
        Consent::Off(why) => format!("off ({why})"),
        Consent::Ask => "not decided (nothing is recorded; `am telemetry on` or `off`)".into(),
    }
}

fn consent_id(c: Consent) -> &'static str {
    match c {
        Consent::On => "on",
        Consent::Off(_) => "off",
        Consent::Ask => "ask",
    }
}

fn install_id_if_made() -> Option<String> {
    std::fs::read_to_string(public::dir().join("install-id")).ok().map(|s| s.trim().to_string())
}

fn ms_ago(ms: u64) -> String {
    let secs = public::now_ms().saturating_sub(ms) / 1000;
    format!("{} ago", am_core::timefmt::short_age(secs as i64))
}

pub fn status(as_json: bool) {
    let s = settings::load();
    let consent = telemetry::consent(&s, |n| std::env::var(n).ok());
    let pending = spool::pending(&public::dir()).len();
    let st = upload::state();
    let endpoint = upload::endpoint();
    if as_json {
        output::json(&json!({
            "consent": consent_id(consent),
            "reason": match consent { Consent::Off(why) => Value::from(why), _ => Value::Null },
            "setting": s.telemetry().id(),
            "endpoint": endpoint,
            "installId": install_id_if_made(),
            "pending": pending,
            "lastUploadMs": st.last_upload_ms,
            "lastError": st.last_error,
        }));
        return;
    }
    output::key_value(&[
        ("Reports".into(), consent_text(consent)),
        ("Sent to".into(), endpoint.unwrap_or_else(|| "nowhere yet (no relay in this build)".into())),
        ("Install id".into(), install_id_if_made().unwrap_or_else(|| "(none yet)".into())),
        ("Waiting to send".into(), format!("{pending} event(s) (`am telemetry show`)")),
        ("Last sent".into(), st.last_upload_ms.map(ms_ago).unwrap_or_else(|| "never".into())),
        ("Last error".into(), st.last_error.unwrap_or_else(|| "-".into())),
    ]);
}

fn set(choice: TelemetryChoice) {
    let mut s = settings::load();
    s.telemetry = choice.id().into();
    if let Err(e) = settings::save(&s) {
        crate::fail(&format!("Could not save settings: {e}"));
    }
    am_core::info!("telemetry", "telemetry set to {} from the CLI", choice.id());
}

pub fn on() {
    set(TelemetryChoice::On);
    println!("Usage reports are on. Thank you!\n\n{ABOUT}\n\n`am telemetry show` lists what is waiting to be sent; `am telemetry off` stops it.");
    if let Consent::Off(why) = telemetry::consent(&settings::load(), |n| std::env::var(n).ok()) {
        println!("\nNote: nothing is recorded here while {why}.");
    }
}

pub fn off() {
    set(TelemetryChoice::Off);
    spool::clear(&public::dir());
    println!("Usage reports are off, and anything waiting to be sent was deleted.");
}

pub fn show(as_json: bool) {
    let events = spool::pending(&public::dir());
    // The real resource (making the random install id if there's none yet),
    // so `--json` is byte for byte what the relay would get.
    let resource = upload::resource();
    if as_json {
        // Exactly the request body the relay gets.
        output::json(&schema::otlp_logs(&events, &resource));
        return;
    }
    if events.is_empty() {
        println!("Nothing waiting to be sent.");
        return;
    }
    println!("Sent with every report:");
    for (k, v) in &resource {
        println!("  {k} = {v}");
    }
    println!("\n{} event(s) waiting:", events.len());
    for e in &events {
        let attrs: Vec<String> = e
            .attrs
            .iter()
            .map(|(k, v)| match v {
                schema::Attr::Str(s) => format!("{k}={s}"),
                schema::Attr::Int(n) => format!("{k}={n}"),
                schema::Attr::Bool(b) => format!("{k}={b}"),
            })
            .collect();
        println!("  {} {} {} v{}  {}", ms_ago(e.ts), e.event, e.binary, e.version, attrs.join(" "));
    }
}

pub fn reset_id() {
    public::reset_install_id();
    println!("Install id forgotten; the next report starts a new one.");
}

pub fn flush() {
    let s = settings::load();
    match telemetry::consent(&s, |n| std::env::var(n).ok()) {
        Consent::On => {}
        other => crate::fail(&format!("Usage reports are {}.", consent_text(other))),
    }
    let Some(endpoint) = upload::endpoint() else {
        crate::fail("No relay is configured in this build; reports stay on this machine (`am telemetry show`).")
    };
    match upload::flush(&endpoint) {
        Ok(upload::Flushed::Sent(n)) => println!("Sent {n} event(s)."),
        Ok(upload::Flushed::Busy) => println!("Another am is sending right now."),
        Err(e) => crate::fail(&format!("Couldn't send: {e}")),
    }
}
