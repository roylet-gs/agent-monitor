//! Sending the spool: [`flush`] moves it into a batch and posts the batches as
//! OTLP/JSON logs to the relay (`<endpoint>/v1/logs`), which adds Grafana
//! Cloud's credentials. One process sends at a time (`upload.lock`, `flock`);
//! a batch is deleted once its requests succeed, and a failed request keeps
//! what hasn't gone yet for the next try. `state.json` remembers the last
//! upload and heartbeat for `am telemetry status` and `am doctor`.

use std::fs;
use std::os::fd::AsRawFd;
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::public::{self, dir, install_id};
use super::schema::{Event, otlp_logs};
use super::spool;
use crate::{debug, info};

const MODULE: &str = "telemetry";
/// The relay that forwards reports to Grafana Cloud (`telemetry/relay`, a
/// Cloudflare Worker on this custom domain). `AM_TELEMETRY_ENDPOINT` overrides it.
pub const DEFAULT_ENDPOINT: Option<&str> = Some("https://am-telemetry.royle.au");
/// Events per request (well under the relay's 256 KB body limit).
const CHUNK: usize = 300;
const TIMEOUT: Duration = Duration::from_secs(10);

/// Where reports go: `AM_TELEMETRY_ENDPOINT`, else [`DEFAULT_ENDPOINT`].
pub fn endpoint() -> Option<String> {
    std::env::var("AM_TELEMETRY_ENDPOINT")
        .ok()
        .map(|v| v.trim().trim_end_matches('/').to_string())
        .filter(|v| !v.is_empty())
        .or(DEFAULT_ENDPOINT.map(str::to_string))
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct State {
    pub last_upload_ms: Option<u64>,
    pub last_error: Option<String>,
    /// UTC day (`2026-10-30`) of the last heartbeat.
    pub last_heartbeat_day: Option<String>,
}

pub fn state() -> State {
    state_in(&dir())
}

pub fn save_state(state: &State) {
    save_state_in(&dir(), state);
}

pub fn state_in(dir: &Path) -> State {
    fs::read_to_string(dir.join("state.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

pub fn save_state_in(dir: &Path, state: &State) {
    let _ = fs::create_dir_all(dir);
    if let Ok(text) = serde_json::to_string_pretty(state) {
        let tmp = dir.join(format!("state.json.{}.tmp", std::process::id()));
        if fs::write(&tmp, text).is_ok() {
            let _ = fs::rename(tmp, dir.join("state.json"));
        }
    }
}

/// The per-install resource attributes sent with every request (makes the
/// install id if there's none yet).
pub fn resource() -> Vec<(&'static str, String)> {
    vec![
        ("os.type", super::os_type().into()),
        ("host.arch", super::host_arch().into()),
        ("am.install_method", install_method().into()),
        ("am.install_id", install_id()),
    ]
}

fn install_method() -> &'static str {
    use crate::update_check::InstallMethod as M;
    match M::current() {
        M::Homebrew => "homebrew",
        M::Npm => "npm",
        M::Pnpm => "pnpm",
        M::Cargo => "cargo",
        M::DevBuild => "dev",
        M::Unknown => "unknown",
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Flushed {
    /// This many events were sent.
    Sent(usize),
    /// Another process holds the lock and is sending.
    Busy,
}

/// Send everything waiting to `endpoint`. Only when reports are on.
pub fn flush(endpoint: &str) -> Result<Flushed, String> {
    if !public::is_on() {
        return Ok(Flushed::Sent(0));
    }
    let result = flush_dir(&dir(), endpoint, &resource(), public::now_ms());
    let mut st = state();
    match &result {
        Ok(Flushed::Sent(n)) => {
            if *n > 0 {
                st.last_upload_ms = Some(public::now_ms());
                info!(MODULE, "Sent {n} usage report event(s)");
            }
            st.last_error = None;
        }
        Ok(Flushed::Busy) => return result,
        Err(e) => {
            debug!(MODULE, "Usage reports not sent: {e}");
            st.last_error = Some(e.clone());
        }
    }
    save_state(&st);
    result
}

/// [`flush`] for a given directory and clock (tests).
pub fn flush_dir(dir: &Path, endpoint: &str, resource: &[(&str, String)], now_ms: u64) -> Result<Flushed, String> {
    let _ = fs::create_dir_all(dir);
    let file = fs::OpenOptions::new().create(true).truncate(false).write(true).open(dir.join("upload.lock")).map_err(|e| e.to_string())?;
    let Some(_lock) = Lock::try_take(file) else { return Ok(Flushed::Busy) };
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(TIMEOUT)).build().into();
    let url = format!("{endpoint}/v1/logs");
    let mut sent = 0;
    for batch in spool::take_batches(dir, now_ms) {
        let events = spool::read(&batch);
        for (i, chunk) in events.chunks(CHUNK).enumerate() {
            if let Err(e) = post(&agent, &url, chunk, resource) {
                // Keep what didn't go for the next try.
                let rest = &events[i * CHUNK..];
                let text: String = rest.iter().filter_map(|e| serde_json::to_string(e).ok()).map(|l| l + "\n").collect();
                let _ = fs::write(&batch, text);
                return Err(e);
            }
            sent += chunk.len();
        }
        let _ = fs::remove_file(&batch);
    }
    Ok(Flushed::Sent(sent))
}

/// An exclusive `flock`, released explicitly on drop: a child process forked
/// by another thread meanwhile holds a copy of the descriptor until it execs,
/// and closing ours alone would leave the lock held through that copy.
struct Lock(fs::File);

impl Lock {
    fn try_take(file: fs::File) -> Option<Lock> {
        (unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0).then_some(Lock(file))
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

fn post(agent: &ureq::Agent, url: &str, events: &[Event], resource: &[(&str, String)]) -> Result<(), String> {
    let body = otlp_logs(events, resource).to_string();
    match agent.post(url).header("content-type", "application/json").send(body) {
        Ok(_) => Ok(()),
        Err(ureq::Error::StatusCode(code)) => Err(format!("{url} answered HTTP {code}")),
        Err(e) => Err(format!("{url}: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;

    /// A relay answering `status` to each request, sending the bodies back.
    fn relay(statuses: Vec<u16>) -> (String, mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for status in statuses {
                let Ok((conn, _)) = listener.accept() else { return };
                let mut reader = BufReader::new(conn.try_clone().unwrap());
                let mut len = 0;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap();
                    }
                    if line == "\r\n" {
                        break;
                    }
                }
                let mut body = vec![0; len];
                reader.read_exact(&mut body).unwrap();
                let _ = tx.send(String::from_utf8(body).unwrap());
                let mut conn = conn;
                write!(conn, "HTTP/1.1 {status} X\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
            }
        });
        (url, rx)
    }

    #[test]
    fn sends_waiting_events_and_deletes_them() {
        let dir = tempfile::tempdir().unwrap();
        spool::append(dir.path(), &Event::command(1, "repo.list", "cli"));
        spool::append(dir.path(), &Event::command(2, "doctor", "mcp"));
        let (url, bodies) = relay(vec![200]);
        let out = flush_dir(dir.path(), &url, &[("am.install_id", "id-1".into())], 10).unwrap();
        assert_eq!(out, Flushed::Sent(2));
        let body: serde_json::Value = serde_json::from_str(&bodies.recv().unwrap()).unwrap();
        let records = &body["resourceLogs"][0]["scopeLogs"][0]["logRecords"];
        assert_eq!(records.as_array().unwrap().len(), 2);
        assert!(spool::pending(dir.path()).is_empty());
    }

    #[test]
    fn a_failed_send_keeps_the_events_for_next_time() {
        let dir = tempfile::tempdir().unwrap();
        spool::append(dir.path(), &Event::command(1, "repo.list", "cli"));
        let (url, _bodies) = relay(vec![503, 200]);
        assert!(flush_dir(dir.path(), &url, &[], 10).unwrap_err().contains("503"));
        assert_eq!(spool::pending(dir.path()).len(), 1);
        assert_eq!(flush_dir(dir.path(), &url, &[], 20).unwrap(), Flushed::Sent(1));
        assert!(spool::pending(dir.path()).is_empty());
    }

    #[test]
    fn only_one_process_sends_at_a_time() {
        let dir = tempfile::tempdir().unwrap();
        let lock = fs::File::create(dir.path().join("upload.lock")).unwrap();
        assert_eq!(unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) }, 0);
        assert_eq!(flush_dir(dir.path(), "http://127.0.0.1:9", &[], 1).unwrap(), Flushed::Busy);
    }

    #[test]
    fn nothing_waiting_sends_nothing() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(flush_dir(dir.path(), "http://127.0.0.1:9", &[], 1).unwrap(), Flushed::Sent(0));
    }
}
