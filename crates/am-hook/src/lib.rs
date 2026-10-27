//! Hook-event handling shared by the `am-hook` binary and `am hook-event`.
//!
//! The agent CLI (Claude Code, Codex, Gemini CLI, Cursor…) runs this on every hook event (at
//! least twice per tool call), so it must start fast: no runtime, no TUI code,
//! one SQLite connection. Usage mirrors the TS command so installed hooks can
//! switch over unchanged:
//!
//!   am-hook hook-event [--agent <id>] --worktree <path> [--event <name>] [--managed]
//!
//! `--agent` (default `claude`) picks the provider that translates the native
//! payload into the canonical event (`am_core::harness`). Never write to stdout
//! or stderr: Gemini shows any hook output to the user.

am_core::silence_unit_tests!();

use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::time::Instant;

use am_core::harness::HarnessKind;
use am_core::db::{self, Owner, StatusUpdate, SubagentKey};
use am_core::status::{self, HookEvent};
use am_core::{debug, info, paths, settings, warn};
use serde_json::Value;

const MODULE: &str = "hook-event";

pub const USAGE: &str = "usage: am-hook hook-event [--agent <id>] --worktree <path> [--event <name>] [--managed]";

/// Parsed `hook-event` flags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookArgs {
    pub worktree: String,
    pub event: Option<String>,
    pub agent: Option<String>,
}

/// Parse the flags that follow the `hook-event` word: `-w/--worktree <path>`,
/// `-e/--event <name>`, `-a/--agent <id>` (alias `--harness`), `--managed`, and the
/// `--flag=value` forms. Unknown
/// flags are ignored so a newer hook command can't break an older binary.
pub fn parse_hook_args(args: impl IntoIterator<Item = String>) -> Result<HookArgs, String> {
    let mut args = args.into_iter();
    let (mut worktree, mut event, mut agent) = (None, None, None);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-w" | "--worktree" => worktree = args.next(),
            "-e" | "--event" => event = args.next(),
            "-a" | "--agent" | "--harness" => agent = args.next(),
            "--managed" => {} // accepted for compatibility with Claude Code's rewritten entries
            other => {
                if let Some(v) = other.strip_prefix("--worktree=") {
                    worktree = Some(v.to_owned());
                } else if let Some(v) = other.strip_prefix("--event=") {
                    event = Some(v.to_owned());
                } else if let Some(v) = other.strip_prefix("--agent=").or_else(|| other.strip_prefix("--harness=")) {
                    agent = Some(v.to_owned());
                }
            }
        }
    }
    Ok(HookArgs { worktree: worktree.ok_or("missing --worktree")?, event, agent })
}

/// Handle one hook event: read the JSON payload from stdin, update SQLite and
/// notify the TUI. Never panics on bad input and never reports failure —
/// hooks must not block or fail the agent, so callers always exit 0.
/// `agent` is the `--agent` id; unknown or missing means Claude.
pub fn run_hook_event(worktree: &str, event: Option<&str>, agent: Option<&str>) {
    let s = settings::load();
    am_core::log::init(s.log_level(), s.max_log_size_mb);
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
    let kind = HarnessKind::from_db(agent);
    if agent.is_some_and(|a| HarnessKind::get(a).is_none()) {
        warn!(MODULE, "Unknown --agent {agent:?}; treating the event as Claude's");
    }
    // Cursor runs Claude Code's hooks too (third-party hooks, on by default);
    // its own hooks (`--agent cursor`) report those sessions.
    if kind == HarnessKind::CLAUDE && payload.get("cursor_version").is_some() {
        debug!(MODULE, "Ignoring a Claude hook fired by Cursor (event {event:?})");
        return;
    }
    let event = kind.provider().normalize(&payload, event);
    maybe_capture(kind, &event, &raw);

    if let Err(e) = handle(worktree, kind, &event) {
        warn!(MODULE, "event={} failed: {e}", event.event);
    }
    let elapsed = started.elapsed();
    debug!(MODULE, "event={} handled in {:.2}ms", event.event, elapsed.as_secs_f64() * 1000.0);
    // One process per event, so no p95 here: every event over budget says so.
    if let Some(budget) = am_core::perf::budget("hook.event")
        && elapsed > budget
    {
        warn!(am_core::perf::MODULE, "hook.event slow: {} > budget {} (event={})", am_core::perf::ms(elapsed), am_core::perf::ms(budget), event.event);
    }
}


fn handle(worktree_arg: &str, kind: HarnessKind, ev: &HookEvent) -> rusqlite::Result<()> {
    info!(
        MODULE,
        "[{}] event={} tool={} agent={} stop_hook_active={} permission_mode={} for {}",
        kind.id(),
        ev.event,
        ev.tool_name.as_deref().unwrap_or("none"),
        ev.agent_id.as_deref().map(|a| format!("{a}({})", ev.agent_type.as_deref().unwrap_or(""))).unwrap_or_else(|| "main".into()),
        ev.stop_hook_active,
        ev.permission_mode.as_deref().unwrap_or("N/A"),
        worktree_arg
    );

    let conn = db::open()?;
    // A harness without a project-dir variable leaves `--worktree ""`: use the payload's cwd.
    let dir = if worktree_arg.trim().is_empty() { ev.cwd.as_deref().unwrap_or("") } else { worktree_arg };
    // Outside git is fine (a standalone session); no directory at all is not.
    if dir.is_empty() {
        debug!(MODULE, "[{}] no directory for event={}; skipped", kind.id(), ev.event);
        return Ok(());
    }
    let root = worktree_root(Path::new(dir)).unwrap_or_else(|| PathBuf::from(dir));
    let root_str = root.to_string_lossy().into_owned();
    let owner = match db::find_worktree(&conn, &root_str)? {
        Some(o) => o,
        None => {
            debug!(MODULE, "No worktree for {root_str}; tracking as standalone session");
            Owner::Standalone { path: root_str.clone() }
        }
    };

    // --- Subagent bookkeeping (independent of whether the status changes) ---
    // The TUI watches the DB (PRAGMA data_version), so no socket notify is needed.
    track_subagents(&conn, &owner, ev)?;

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

    // Each session keeps its own status, so two sessions in one worktree
    // don't overwrite each other; the owner's row is their headline.
    if let Some(sid) = ev.session_id.as_deref() {
        let session = db::get_session(&conn, kind, sid)?;
        let running = db::running_subagents_in(&conn, &owner, sid)?;
        let current_status = session.as_ref().map(|s| s.status);
        match status::map_event_to_status(ev, current_status, running) {
            Some(new_status) => {
                let unchanged = session.as_ref().is_some_and(|s| {
                    s.status == new_status && s.is_open == is_open && last_response.is_none()
                });
                if unchanged {
                    db::touch_session(&conn, &owner, kind, sid)?;
                } else {
                    db::upsert_session(&conn, &owner, kind, sid, new_status, last_response, is_open)?;
                    info!(
                        MODULE,
                        "Session {}:{sid} in {root_str}: {} (was {:?}, subagents={running})",
                        kind.id(),
                        new_status.as_str(),
                        current_status
                    );
                }
            }
            None => {
                debug!(MODULE, "No status change for session {sid} in {root_str} ({}, current={:?})", ev.event, current_status);
                if session.is_some() {
                    db::touch_session(&conn, &owner, kind, sid)?;
                }
            }
        }
        refresh_headline(&conn, &owner, ev, &root_str)?;
    } else {
        // No session id (very old CLIs): status the owner directly, as before v4.
        let current = db::get_status(&conn, &owner)?;
        let current_status = current.as_ref().and_then(|c| c.status);
        let running = db::running_subagents(&conn, &owner)?;
        if let Some(new_status) = status::map_event_to_status(ev, current_status, running) {
            db::upsert_status(
                &conn,
                &owner,
                &StatusUpdate {
                    status: new_status,
                    agent: Some(kind),
                    session_id: None,
                    last_response,
                    transcript_summary: ev.transcript_summary.as_deref(),
                    is_open,
                },
            )?;
            info!(MODULE, "Updated status for {root_str}: {} (was {:?}, no session id)", new_status.as_str(), current_status);
        }
    }

    if ev.is("Stop") || ev.is("SessionEnd") {
        db::checkpoint_passive(&conn);
    }
    Ok(())
}

/// Mirror the most urgent open session into the owner's `agent_status` /
/// `standalone_sessions` row (what 1.x, filters, sort and the CLI read), plus
/// the owner-wide live subagent count. With no open session, the latest one.
fn refresh_headline(conn: &rusqlite::Connection, owner: &Owner, ev: &HookEvent, root_str: &str) -> rusqlite::Result<()> {
    let sessions = db::sessions_at(conn, owner)?;
    let Some(head) = status::headline(&sessions, |s| (s.is_open, s.status)).cloned() else { return Ok(()) };
    let running = db::running_subagents(conn, owner)?;
    let current = db::get_status(conn, owner)?;
    if current.as_ref().map(|c| c.active_subagents) != Some(running as i64) {
        db::set_active_subagents(conn, owner, running)?;
        debug!(MODULE, "Running subagents for {root_str}: {running}");
    }
    // The summary only belongs to the session that sent it.
    let summary = ev.transcript_summary.as_deref().filter(|_| ev.session_id.as_deref() == Some(head.session_id.as_str()));
    let unchanged = current.as_ref().is_some_and(|c| {
        c.status == Some(head.status)
            && c.agent == head.agent
            && c.is_open == head.is_open
            && c.session_id.as_deref() == Some(head.session_id.as_str())
            && summary.is_none()
    });
    if unchanged {
        return db::touch_status(conn, owner);
    }
    db::upsert_status(
        conn,
        owner,
        &StatusUpdate {
            status: head.status,
            agent: Some(head.agent),
            session_id: Some(&head.session_id),
            last_response: head.last_response.as_deref(),
            transcript_summary: summary,
            is_open: head.is_open,
        },
    )?;
    let open = sessions.iter().filter(|s| s.is_open).count();
    info!(
        MODULE,
        "Updated status for {root_str}: {} from session {}:{} ({open} open, was {:?}, subagents={running})",
        head.status.as_str(),
        head.agent.id(),
        head.session_id,
        current.and_then(|c| c.status)
    );
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
        // PreToolUse records what the subagent is doing now; PostToolUse only
        // proves it is still alive (keeps it out of the stale window).
        "PreToolUse" => {
            let tool = ev.tool_name.as_deref();
            let input = tool.and_then(|t| status::summarize_tool_input(t, ev.tool_input.as_ref(), ev.cwd.as_deref()));
            db::subagent_tool(conn, owner, &key, agent_type, tool, input.as_deref(), true)?;
        }
        "PostToolUse" => db::subagent_tool(conn, owner, &key, agent_type, None, None, false)?,
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

/// `AM_HOOK_CAPTURE=1` saves raw payloads (one file per event name, newest
/// wins) so field names can be verified against what each agent really sends.
/// Claude's files keep their old names; other agents' get an `<id>-` prefix.
fn maybe_capture(kind: HarnessKind, ev: &HookEvent, raw: &str) {
    if std::env::var_os("AM_HOOK_CAPTURE").is_none_or(|v| v.is_empty() || v == "0") {
        return;
    }
    let dir = paths::capture_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let origin = if ev.from_subagent() { "subagent" } else { "main" };
    let prefix = if kind == HarnessKind::CLAUDE { String::new() } else { format!("{}-", kind.id()) };
    let _ = std::fs::write(dir.join(format!("{prefix}{}-{origin}.json", ev.event)), raw);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> HookArgs {
        parse_hook_args(args.iter().map(|s| s.to_string())).unwrap()
    }

    #[test]
    fn harness_is_an_alias_for_agent() {
        assert_eq!(parse(&["--harness", "codex", "-w", "/r"]).agent.as_deref(), Some("codex"));
        assert_eq!(parse(&["--harness=codex", "-w", "/r"]).agent.as_deref(), Some("codex"));
        assert_eq!(parse(&["--agent", "gemini", "-w", "/r"]).agent.as_deref(), Some("gemini"));
    }

    #[test]
    fn unknown_flags_are_ignored_and_worktree_is_required() {
        assert_eq!(parse(&["--future", "-w", "/r", "-e", "Stop"]).event.as_deref(), Some("Stop"));
        assert!(parse_hook_args(["--event".to_string(), "Stop".to_string()]).is_err());
    }
}
