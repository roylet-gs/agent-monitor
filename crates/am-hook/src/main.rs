//! `am-hook` — the native replacement for `am hook-event`.
//!
//! Claude Code runs this on every hook event (at least twice per tool call),
//! so it must start fast: no runtime, no TUI code, one SQLite connection.
//! Usage mirrors the TS command so installed hooks can switch over unchanged:
//!
//!   am-hook hook-event --worktree <path> [--event <name>] [--managed]

use std::io::{IsTerminal, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use am_core::db::{self, Owner, StatusUpdate, SubagentKey};
use am_core::status::{self, HookEvent, Status};
use am_core::{debug, info, paths, settings, warn};
use serde_json::{Value, json};

const MODULE: &str = "hook-event";

struct Args {
    worktree: String,
    event: Option<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("hook-event") => {}
        Some("--version") | Some("-V") => {
            println!("am-hook {}", env!("CARGO_PKG_VERSION"));
            std::process::exit(0);
        }
        _ => return Err("usage: am-hook hook-event --worktree <path> [--event <name>] [--managed]".into()),
    }
    let (mut worktree, mut event) = (None, None);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-w" | "--worktree" => worktree = args.next(),
            "-e" | "--event" => event = args.next(),
            "--managed" => {} // accepted for compatibility with Claude Code's rewritten entries
            other => {
                if let Some(v) = other.strip_prefix("--worktree=") {
                    worktree = Some(v.to_owned());
                } else if let Some(v) = other.strip_prefix("--event=") {
                    event = Some(v.to_owned());
                }
            }
        }
    }
    Ok(Args { worktree: worktree.ok_or("missing --worktree")?, event })
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let s = settings::load();
    am_core::log::init(s.log_level, s.max_log_size_mb);
    let started = Instant::now();

    let mut raw = String::new();
    if !std::io::stdin().is_terminal() {
        let _ = std::io::stdin().read_to_string(&mut raw);
    }
    let payload: Value = if raw.trim().is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&raw).unwrap_or_else(|_| {
            warn!(MODULE, "Failed to parse stdin JSON ({} bytes)", raw.len());
            Value::Null
        })
    };
    let event = HookEvent::from_json(&payload, args.event.as_deref());
    maybe_capture(&event, &raw);

    if let Err(e) = handle(&args.worktree, &event) {
        warn!(MODULE, "event={} failed: {e}", event.event);
    }
    debug!(MODULE, "event={} handled in {:.2}ms", event.event, started.elapsed().as_secs_f64() * 1000.0);
    // Hooks must never block or fail Claude Code: always exit 0.
}

fn handle(worktree_arg: &str, ev: &HookEvent) -> rusqlite::Result<()> {
    info!(
        MODULE,
        "event={} tool={} agent={} stop_hook_active={} permission_mode={} for {}",
        ev.event,
        ev.tool_name.as_deref().unwrap_or("none"),
        ev.agent_id.as_deref().map(|a| format!("{a}({})", ev.agent_type.as_deref().unwrap_or(""))).unwrap_or_else(|| "main".into()),
        ev.stop_hook_active,
        ev.permission_mode.as_deref().unwrap_or("N/A"),
        worktree_arg
    );

    let conn = db::open()?;
    let root = worktree_root(Path::new(worktree_arg)).unwrap_or_else(|| PathBuf::from(worktree_arg));
    let root_str = root.to_string_lossy().into_owned();
    let owner = match db::find_worktree(&conn, &root_str)? {
        Some(o) => o,
        None => {
            debug!(MODULE, "No worktree for {root_str}; tracking as standalone session");
            Owner::Standalone { path: root_str.clone() }
        }
    };

    let current = db::get_status(&conn, &owner)?;
    let current_status = current.as_ref().and_then(|c| c.status);

    // --- Subagent bookkeeping (independent of whether the status changes) ---
    let subagents_changed = track_subagents(&conn, &owner, ev)?;
    let running = db::running_subagents(&conn, &owner)?;
    if current.as_ref().map(|c| c.active_subagents) != Some(running as i64) {
        db::set_active_subagents(&conn, &owner, running)?;
        debug!(MODULE, "Running subagents for {root_str}: {running}");
    }
    if subagents_changed {
        publish(&json!({ "type": "subagent-update", "owner": owner_json(&owner), "running": running }));
    }

    let Some(new_status) = status::map_event_to_status(ev, current_status, running) else {
        debug!(MODULE, "No status change for {root_str} ({}, current={:?})", ev.event, current_status);
        return Ok(());
    };

    let last_response = if ev.from_subagent() {
        None
    } else if let Some(m) = ev.last_assistant_message.as_deref().filter(|_| !ev.is_subagent_lifecycle()) {
        Some(m)
    } else if ev.is("Notification") {
        ev.message.as_deref()
    } else {
        None
    };
    let is_open = !ev.is("SessionEnd");
    let session_id = if ev.from_subagent() { None } else { ev.session_id.as_deref() };

    let unchanged = current.as_ref().is_some_and(|c| {
        c.status == Some(new_status)
            && c.is_open == is_open
            && last_response.is_none()
            && ev.transcript_summary.is_none()
            && (session_id.is_none() || session_id == c.session_id.as_deref())
    });
    if unchanged {
        db::touch_status(&conn, &owner)?;
        return Ok(());
    }

    db::upsert_status(
        &conn,
        &owner,
        &StatusUpdate {
            status: new_status,
            session_id,
            last_response,
            transcript_summary: ev.transcript_summary.as_deref(),
            is_open,
        },
    )?;
    info!(MODULE, "Updated status for {root_str}: {} (was {:?}, subagents={running})", new_status.as_str(), current_status);
    publish_status(&owner, new_status, session_id, last_response, ev.transcript_summary.as_deref(), is_open);

    if ev.is("PostToolUse") && !ev.from_subagent() {
        let cmd = ev.tool_input.as_ref().and_then(|t| t.get("command")).and_then(Value::as_str).unwrap_or("");
        if let (Some(activity), Owner::Worktree { id, repo_id, branch }) = (status::detect_git_activity(cmd), &owner) {
            info!(MODULE, "Detected git activity: {activity} for {root_str}");
            publish(&json!({ "type": "git-activity", "worktreeId": id, "repoId": repo_id, "branch": branch, "activity": activity }));
        }
    }

    if ev.is("Stop") || ev.is("SessionEnd") {
        db::checkpoint_passive(&conn);
    }
    Ok(())
}

/// Update the `subagents` table for this event. Returns true when a row changed.
fn track_subagents(conn: &rusqlite::Connection, owner: &Owner, ev: &HookEvent) -> rusqlite::Result<bool> {
    let Some(session_id) = ev.session_id.as_deref() else { return Ok(false) };

    if ev.is("SessionStart") {
        let n = db::end_subagents(conn, owner, session_id, true)?;
        if n > 0 {
            debug!(MODULE, "SessionStart closed {n} subagent(s) from earlier sessions");
        }
        return Ok(n > 0);
    }
    if ev.is("SessionEnd") {
        return Ok(db::end_subagents(conn, owner, session_id, false)? > 0);
    }

    let Some(agent_id) = ev.agent_id.as_deref() else { return Ok(false) };
    if ev.is_internal_agent() {
        debug!(MODULE, "Ignoring internal agent {agent_id} ({})", ev.event);
        return Ok(false);
    }
    let key = SubagentKey { session_id, agent_id };
    let agent_type = ev.agent_type.as_deref().filter(|t| !t.is_empty());

    match ev.event.as_str() {
        "SubagentStart" => db::subagent_started(conn, owner, &key, agent_type)?,
        "SubagentStop" => db::subagent_stopped(
            conn,
            owner,
            &key,
            agent_type,
            ev.agent_transcript_path.as_deref(),
            ev.last_assistant_message.as_deref().map(|m| status::truncate(m.trim(), 500)).as_deref(),
        )?,
        "PreToolUse" | "PostToolUse" => {
            let tool = ev.tool_name.as_deref();
            let input = tool.and_then(|t| status::summarize_tool_input(t, ev.tool_input.as_ref()));
            db::subagent_tool(conn, owner, &key, agent_type, tool, input.as_deref(), ev.is("PreToolUse"))?;
        }
        _ => return Ok(false),
    }

    if !db::subagent_has_description(conn, &key)? {
        enrich_from_meta(conn, &key, ev);
    }
    Ok(true)
}

/// Claude Code writes `<session>/subagents/agent-<id>.meta.json` next to each
/// subagent transcript (undocumented, so best effort): it carries the Agent
/// tool's description, which is what the TUI shows as the subagent's name.
fn enrich_from_meta(conn: &rusqlite::Connection, key: &SubagentKey, ev: &HookEvent) {
    let transcript = ev
        .agent_transcript_path
        .as_deref()
        .map(PathBuf::from)
        .or_else(|| {
            let main = Path::new(ev.transcript_path.as_deref()?);
            let session_dir = main.with_extension("");
            Some(session_dir.join("subagents").join(format!("agent-{}.jsonl", key.agent_id)))
        });
    let Some(transcript) = transcript else { return };
    let meta: Option<Value> = std::fs::read_to_string(transcript.with_extension("meta.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok());
    let description = meta.as_ref().and_then(|m| m.get("description")).and_then(Value::as_str);
    let agent_type = meta.as_ref().and_then(|m| m.get("agentType")).and_then(Value::as_str);
    let transcript_str = transcript.exists().then(|| transcript.to_string_lossy().into_owned());
    if description.is_some() || transcript_str.is_some() {
        let _ = db::set_subagent_meta(conn, key, description, agent_type, transcript_str.as_deref());
    }
}

/// Walk up to the directory containing `.git` (a dir for the main checkout, a
/// file for linked worktrees). Replaces a `git rev-parse` spawn per event.
fn worktree_root(start: &Path) -> Option<PathBuf> {
    let start = start.canonicalize().ok()?;
    start.ancestors().find(|p| p.join(".git").exists()).map(Path::to_path_buf)
}

fn owner_json(owner: &Owner) -> Value {
    match owner {
        Owner::Worktree { id, .. } => json!({ "worktreeId": id }),
        Owner::Standalone { path } => json!({ "sessionPath": path }),
    }
}

fn publish_status(owner: &Owner, status: Status, session_id: Option<&str>, last_response: Option<&str>, summary: Option<&str>, is_open: bool) {
    let now = time::OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339).unwrap_or_default();
    let mut msg = json!({
        "status": status.as_str(),
        "sessionId": session_id,
        "lastResponse": last_response,
        "transcriptSummary": summary,
        "isOpen": is_open,
        "updatedAt": now,
    });
    match owner {
        Owner::Worktree { id, .. } => {
            msg["type"] = "agent-status-update".into();
            msg["worktreeId"] = id.as_str().into();
        }
        Owner::Standalone { path } => {
            msg["type"] = "standalone-status-update".into();
            msg["sessionPath"] = path.as_str().into();
        }
    }
    publish(&msg);
}

/// Fire-and-forget notify of the TUI over the Unix socket. SQLite is the
/// source of truth; a missing or busy socket is not an error.
fn publish(msg: &Value) {
    let Ok(mut stream) = UnixStream::connect(paths::socket_path()) else { return };
    let _ = stream.set_write_timeout(Some(Duration::from_millis(200)));
    let mut line = msg.to_string();
    line.push('\n');
    let _ = stream.write_all(line.as_bytes());
}

/// `AM_HOOK_CAPTURE=1` saves raw payloads (one file per event name, newest
/// wins) so field names can be verified against what Claude Code really sends.
fn maybe_capture(ev: &HookEvent, raw: &str) {
    if std::env::var_os("AM_HOOK_CAPTURE").is_none_or(|v| v.is_empty() || v == "0") {
        return;
    }
    let dir = paths::capture_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let origin = if ev.from_subagent() { "subagent" } else { "main" };
    let _ = std::fs::write(dir.join(format!("{}-{origin}.json", ev.event)), raw);
}
