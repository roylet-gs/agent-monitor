//! OpenCode (sst/opencode) as a built-in plugin: the manifest `opencode.json`,
//! a generated JS plugin in place of hooks, and session discovery from
//! OpenCode's SQLite database.
//!
//! OpenCode has no shell hooks; it loads JS/TS plugins and hands them its
//! event bus. `am hooks install --agent opencode` writes
//! `<config dir>/plugins/agent-monitor.js` (an [`OwnedFile`] carrying
//! [`OWNED_FILE_MARKER`], rendered from `opencode-plugin.js`). OpenCode
//! auto-loads every `{plugin,plugins}/*.{js,ts}` in its config directories,
//! so nothing is registered in `opencode.json`. The plugin goes in the
//! global config dir, `$XDG_CONFIG_HOME/opencode`, else `~/.config/opencode`,
//! which OpenCode always loads. `$OPENCODE_CONFIG_DIR` is not used: OpenCode
//! loads it *as well as* the global dir and dedupes plugins by file URL, so
//! a copy there would run every hook twice next to a global one, and would
//! be missed by an OpenCode started without the variable. OpenCode counts as
//! present when that global dir exists (`with_presence`), so an XDG config
//! dir is detected too.
//!
//! The plugin turns bus events into canonical events (the manifest's events
//! map is the identity) and runs am-hook for each, one at a time, detached,
//! with a 5 s kill, never throwing into OpenCode:
//! - `session.created` → `SessionStart`; archived (`session.updated` with
//!   `time.archived`) or `session.deleted` → `SessionEnd`.
//! - `session.status` busy (and V2's `session.next.prompted` /
//!   `session.execution.started`) → `UserPromptSubmit`, once per turn.
//! - `session.status` idle / `session.idle` (OpenCode sends both) /
//!   `session.execution.succeeded|interrupted` → `Stop`, once per turn, since
//!   a second `Stop` would turn `done` into `idle`. An interruption by
//!   OpenCode's own shutdown is resumed, so it isn't a stop.
//! - `session.error` / `session.execution.failed` → `StopFailure`, except
//!   `MessageAbortedError` (Esc; the idle after it ends the turn) and
//!   `ContextOverflowError` (OpenCode compacts and goes on).
//! - `permission.asked` (V1 `permission.updated`) → `PermissionRequest`
//!   (waiting); the last `permission.replied` → `PostToolUse` (executing). A
//!   turn that ends with a request still open (an interrupt) sends
//!   `PostToolUse` before its `Stop`, since `Stop` keeps `waiting`.
//! - `tool.execute.before|after` hooks → `PreToolUse` / `PostToolUse` with
//!   the tool name (mapped onto Claude's) and only the arguments am
//!   summarises (`command`, `filePath` → `file_path`, `pattern`…).
//! - A child session (`parentID`, made by the `task` tool) is a subagent of
//!   its parent: its start/busy → `SubagentStart`, its idle/error/delete →
//!   `SubagentStop`, its tool calls and permission requests are sent with
//!   `session_id` = the parent and `agent_id` = the child, so it never counts
//!   as a session of its own.
//!
//! Each plugin export must be a function (OpenCode calls every export), so
//! the file exports only `AgentMonitor`. Resume: `opencode --session <id>`,
//! `opencode --continue`. Sessions come from `$OPENCODE_DB`, else
//! `$XDG_DATA_HOME/opencode/opencode.db`, else
//! `~/.local/share/opencode/opencode.db` (table `session`, read-only;
//! top-level, unarchived), falling back to the sessions am's hooks recorded
//! for versions that keep sessions as JSON files.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::embedded::{Embedded, recorded_sessions};
use super::manifest::{Manifest, expand_home};
use super::{HarnessKind, is_within};
use crate::hooks_installer::{HookContext, HookInstall, OWNED_FILE_MARKER, OwnedFile, js_string};
use crate::launch::Session;

pub static OPENCODE: Embedded = Embedded::new(|| Manifest::builtin(include_str!("opencode.json")), Some(discover))
    .with_hook_file(|| (plugin_path(), ""))
    .with_presence(|| config_dir(&env).is_dir())
    .with_hook_install(|| HookInstall::OwnedFile(OwnedFile { render, register: None }));

const TEMPLATE: &str = include_str!("opencode-plugin.js");
const ARGV_SLOT: &str = "__AM_HOOK_ARGV__";
const COMMAND_SLOT: &str = "__AM_HOOK_COMMAND__";

/// The plugin file am owns.
pub fn plugin_path() -> PathBuf {
    config_dir(&env).join("plugins").join("agent-monitor.js")
}

fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().map(|v| v.trim().to_owned()).filter(|v| !v.is_empty())
}

/// OpenCode's global config dir, which it always loads:
/// `$XDG_CONFIG_HOME/opencode`, else `~/.config/opencode`. Never
/// `$OPENCODE_CONFIG_DIR` (see the module docs).
fn config_dir(get: &dyn Fn(&str) -> Option<String>) -> PathBuf {
    match get("XDG_CONFIG_HOME") {
        Some(x) => expand_home(&x).join("opencode"),
        None => expand_home("~/.config/opencode"),
    }
}

/// OpenCode's session database: `$OPENCODE_DB`,
/// `$XDG_DATA_HOME/opencode/opencode.db`, `~/.local/share/opencode/opencode.db`.
fn db_path(get: &dyn Fn(&str) -> Option<String>) -> PathBuf {
    if let Some(p) = get("OPENCODE_DB") {
        return expand_home(&p);
    }
    match get("XDG_DATA_HOME") {
        Some(x) => expand_home(&x).join("opencode/opencode.db"),
        None => expand_home("~/.local/share/opencode/opencode.db"),
    }
}

/// The plugin's source: the template with am-hook's argv up to `--event`
/// filled in (the plugin appends the event name).
fn render(ctx: &HookContext) -> String {
    let argv = ctx.argv("", "");
    let head = &argv[..argv.len() - 1];
    let list = head.iter().map(|a| js_string(a)).collect::<Vec<_>>().join(", ");
    // The shell form names the program next to `hook-event`, which is how
    // the dangling-binary scan finds it whatever the binary is called.
    let command = crate::harness::hook_command_line(&ctx.agent_id, None, "<event>", &ctx.invocation);
    debug_assert!(TEMPLATE.contains(OWNED_FILE_MARKER));
    TEMPLATE.replace(COMMAND_SLOT, &command).replace(ARGV_SLOT, &list)
}

fn discover(root: &Path) -> Vec<Session> {
    let db = db_path(&env);
    match sessions_in_db(&db, root) {
        Some(sessions) => sessions,
        None => recorded_sessions(HarnessKind::OPENCODE, root),
    }
}

/// Top-level, unarchived sessions started at `root` or below, newest first;
/// `None` when the database can't be read (missing, another schema).
pub fn sessions_in_db(db: &Path, root: &Path) -> Option<Vec<Session>> {
    use rusqlite::{Connection, OpenFlags};
    if !db.is_file() {
        return None;
    }
    let conn = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).ok()?;
    let _ = conn.busy_timeout(Duration::from_millis(500));
    let mut stmt = conn
        .prepare(
            "SELECT id, directory, time_updated FROM session \
             WHERE parent_id IS NULL AND time_archived IS NULL ORDER BY time_updated DESC LIMIT 500",
        )
        .map_err(|e| crate::debug!("agents", "opencode: session table unreadable in {}: {e}", db.display()))
        .ok()?;
    let root = root.to_string_lossy();
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)))
        .ok()?
        .flatten()
        .filter(|(_, dir, _)| is_within(dir, &root))
        .map(|(id, cwd, ms)| Session {
            id,
            cwd,
            modified: SystemTime::UNIX_EPOCH + Duration::from_millis(ms.max(0) as u64),
            agent: HarnessKind::OPENCODE,
        })
        .collect();
    Some(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::Harness;
    use crate::status::{Status::*, map_event_to_status};
    use serde_json::{Value, json};

    fn ctx(native: Option<&Path>) -> HookContext {
        HookContext::new(&OPENCODE, native)
    }

    #[test]
    fn the_plugin_spawns_am_hook_with_the_exact_argv() {
        let text = render(&ctx(Some(Path::new("/opt/am bin/am-hook"))));
        assert!(text.contains(OWNED_FILE_MARKER));
        assert!(!text.contains(ARGV_SLOT) && !text.contains(COMMAND_SLOT));
        assert!(text.contains("// \"/opt/am bin/am-hook\" hook-event --agent opencode --worktree \"\" --event <event>\n"), "{text}");
        let head = r#"const HOOK = ["/opt/am bin/am-hook", "hook-event", "--agent", "opencode", "--worktree", "", "--event"];"#;
        assert!(text.contains(head), "{text}");
        // Its only export is the plugin function (OpenCode calls every export).
        assert_eq!(text.matches("export ").count(), 1);
        // The CLI fallback runs `am hook-event`, which also reads stdin.
        assert!(render(&ctx(None)).contains(r#"const HOOK = ["am", "hook-event", "--agent", "opencode""#));
        // The argv it builds is HookContext::argv's, the event appended.
        let c = ctx(Some(Path::new("/b/am-hook")));
        assert_eq!(c.argv("Stop", ""), ["/b/am-hook", "hook-event", "--agent", "opencode", "--worktree", "", "--event", "Stop"]);
        // The dangling-binary scan finds the program in the file.
        let mut found = crate::hooks_installer::hook_programs_in_text(&render(&c));
        found.dedup();
        assert_eq!(found, [PathBuf::from("/b/am-hook")]);
        // Also when the binary has another name ($AM_HOOK_BIN).
        let odd = render(&ctx(Some(Path::new("/x/my-hook"))));
        assert!(crate::hooks_installer::hook_programs_in_text(&odd).contains(&PathBuf::from("/x/my-hook")));
    }

    #[test]
    fn the_plugin_parses_as_an_es_module() {
        let Ok(out) = std::process::Command::new("node").arg("--version").output() else { return };
        if !out.status.success() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("agent-monitor.mjs");
        std::fs::write(&file, render(&ctx(Some(Path::new("/b/am-hook"))))).unwrap();
        let check = std::process::Command::new("node").arg("--check").arg(&file).output().unwrap();
        assert!(check.status.success(), "{}", String::from_utf8_lossy(&check.stderr));
    }

    /// Runs the rendered plugin under node against a fake am-hook that logs
    /// its argv and stdin, feeding it OpenCode's events. Skipped without node.
    #[test]
    fn the_plugin_sends_one_hook_per_state_change_in_order() {
        use std::os::unix::fs::PermissionsExt;
        if !std::process::Command::new("node").arg("--version").output().is_ok_and(|o| o.status.success()) {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("hooks.log");
        let hook = dir.path().join("am-hook");
        std::fs::write(&hook, format!("#!/bin/sh\n{{ printf '%s ' \"$@\"; printf '\\t'; cat; printf '\\n'; }} >> '{}'\n", log.display())).unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        let plugin = dir.path().join("agent-monitor.mjs");
        std::fs::write(&plugin, render(&ctx(Some(&hook)))).unwrap();
        let driver = dir.path().join("driver.mjs");
        std::fs::write(
            &driver,
            r#"import * as fs from "node:fs";
const [plugin, log] = process.argv.slice(2);
const { AgentMonitor } = await import(plugin);
const h = await AgentMonitor({ directory: "/w/repo" });
const ev = (type, properties) => h.event({ event: { type, properties } });
const v2 = (type, data) => h.event({ event: { type, data } });
await ev("session.created", { info: { id: "ses_1", directory: "/w/repo", title: "New session" } });
await ev("session.status", { sessionID: "ses_1", status: { type: "busy" } });
await ev("session.status", { sessionID: "ses_1", status: { type: "busy" } });
await h["tool.execute.before"]({ tool: "bash", sessionID: "ses_1", callID: "c1" }, { args: { command: "cargo test", content: "x".repeat(9000) } });
await ev("permission.asked", { id: "per_1", sessionID: "ses_1", permission: "bash", patterns: ["cargo test"] });
await ev("permission.replied", { sessionID: "ses_1", requestID: "per_1", reply: "once" });
await h["tool.execute.after"]({ tool: "bash", sessionID: "ses_1", callID: "c1", args: { command: "cargo test" } }, { title: "", output: "ok", metadata: {} });
await ev("session.created", { info: { id: "ses_2", parentID: "ses_1", directory: "/w/repo", title: "Look around (@explore subagent)" } });
await ev("session.status", { sessionID: "ses_2", status: { type: "busy" } });
await h["tool.execute.before"]({ tool: "read", sessionID: "ses_2", callID: "c2" }, { args: { filePath: "/w/repo/a.rs" } });
await ev("session.status", { sessionID: "ses_2", status: { type: "idle" } });
await ev("session.idle", { sessionID: "ses_2" });
await ev("session.error", { sessionID: "ses_1", error: { name: "MessageAbortedError", data: {} } });
await ev("session.status", { sessionID: "ses_1", status: { type: "idle" } });
await ev("session.idle", { sessionID: "ses_1" });
await v2("session.next.prompted", { sessionID: "ses_1" });
await ev("permission.asked", { id: "per_2", sessionID: "ses_1", permission: "edit" });
await v2("session.execution.failed", { sessionID: "ses_1", error: "boom" });
await ev("session.idle", { sessionID: "ses_1" });
await ev("session.updated", { info: { id: "ses_1", directory: "/w/repo", time: { archived: 1 } } });
await ev("todo.updated", { sessionID: "ses_1", todos: [] });
await h.event({});
const want = 15;
const start = Date.now();
const lines = () => { try { return fs.readFileSync(log, "utf8").split("\n").filter(Boolean).length; } catch { return 0; } };
while (lines() < want && Date.now() - start < 8000) await new Promise((r) => setTimeout(r, 25));
await new Promise((r) => setTimeout(r, 200));
"#,
        )
        .unwrap();
        let out = std::process::Command::new("node").arg(&driver).arg(&plugin).arg(&log).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let text = std::fs::read_to_string(&log).unwrap();
        let sent: Vec<(String, Value)> = text
            .lines()
            .map(|l| {
                let (args, payload) = l.split_once('\t').unwrap();
                let args = args.trim_end();
                assert!(args.starts_with("hook-event --agent opencode --worktree  --event "), "{args}");
                (args.rsplit(' ').next().unwrap().to_owned(), serde_json::from_str(payload).unwrap())
            })
            .collect();
        let events: Vec<&str> = sent.iter().map(|(e, _)| e.as_str()).collect();
        assert_eq!(
            events,
            [
                "SessionStart",
                "UserPromptSubmit",
                "PreToolUse",
                "PermissionRequest",
                "PostToolUse",
                "PostToolUse",
                "SubagentStart",
                "PreToolUse",
                "SubagentStop",
                "Stop",
                "UserPromptSubmit",
                "PermissionRequest",
                "PostToolUse",
                "StopFailure",
                "SessionEnd",
            ]
        );
        for (event, p) in &sent {
            assert_eq!(p["hook_event_name"], event.as_str());
            assert_eq!(p["session_id"], "ses_1", "{p}");
            assert_eq!(p["cwd"], "/w/repo");
        }
        assert_eq!(sent[2].1["tool_name"], "bash");
        assert_eq!(sent[2].1["tool_input"], json!({"command": "cargo test"}), "only what am summarises");
        assert_eq!(sent[3].1["tool_name"], "bash");
        assert!(sent[4].1.get("tool_name").is_none() && sent[4].1.get("agent_id").is_none());
        for i in [6, 7, 8] {
            assert_eq!((&sent[i].1["agent_id"], &sent[i].1["agent_type"]), (&json!("ses_2"), &json!("explore")), "{}", sent[i].1);
        }
        assert_eq!(sent[7].1["tool_input"], json!({"file_path": "/w/repo/a.rs"}));
        assert!(sent[0].1.get("agent_id").is_none() && sent[9].1.get("agent_id").is_none());
    }

    #[test]
    fn config_and_db_paths_follow_opencode_overrides() {
        let home = crate::paths::home_dir();
        let none = |_: &str| None;
        assert_eq!(config_dir(&none), home.join(".config/opencode"));
        assert_eq!(db_path(&none), home.join(".local/share/opencode/opencode.db"));
        let xdg = |k: &str| match k {
            "XDG_CONFIG_HOME" => Some("/x/cfg".to_owned()),
            "XDG_DATA_HOME" => Some("/x/data".to_owned()),
            _ => None,
        };
        assert_eq!(config_dir(&xdg), PathBuf::from("/x/cfg/opencode"));
        assert_eq!(db_path(&xdg), PathBuf::from("/x/data/opencode/opencode.db"));
        let own = |k: &str| match k {
            "OPENCODE_CONFIG_DIR" => Some("/oc".to_owned()),
            "OPENCODE_DB" => Some("/oc/sessions.db".to_owned()),
            "XDG_CONFIG_HOME" => Some("/x/cfg".to_owned()),
            _ => None,
        };
        // $OPENCODE_CONFIG_DIR is an extra dir OpenCode loads beside the
        // global one; installing there could load the plugin twice.
        assert_eq!(config_dir(&own), PathBuf::from("/x/cfg/opencode"));
        assert_eq!(db_path(&own), PathBuf::from("/oc/sessions.db"));
    }

    #[test]
    fn the_plugins_payloads_drive_the_status_rules() {
        let step = |cur, p: Value| map_event_to_status(&OPENCODE.normalize(&p, None), cur, 0);
        let p = |event: &str| json!({"session_id": "ses_1", "cwd": "/w", "hook_event_name": event});
        let e = OPENCODE.normalize(&p("UserPromptSubmit"), None);
        assert_eq!((e.event.as_str(), e.session_id.as_deref(), e.cwd.as_deref()), ("UserPromptSubmit", Some("ses_1"), Some("/w")));
        assert_eq!(step(None, p("SessionStart")), Some(Idle));
        let s = step(Some(Idle), p("UserPromptSubmit"));
        assert_eq!(s, Some(Executing));
        let mut tool = p("PreToolUse");
        tool["tool_name"] = "bash".into();
        tool["tool_input"] = json!({"command": "cargo test"});
        let e = OPENCODE.normalize(&tool, None);
        assert_eq!(e.tool_name.as_deref(), Some("Bash"));
        let s = step(s, p("PermissionRequest"));
        assert_eq!(s, Some(Waiting));
        let s = step(s, p("PostToolUse"));
        assert_eq!(s, Some(Executing));
        assert_eq!(step(s, p("Stop")), Some(Done));
        assert_eq!(step(s, p("StopFailure")), Some(Idle));
        // A subagent's tool call doesn't move the parent; its permission prompt does.
        let mut sub = p("PreToolUse");
        sub["agent_id"] = "ses_child".into();
        sub["agent_type"] = "explore".into();
        assert_eq!(step(Some(Executing), sub.clone()), None);
        sub["hook_event_name"] = "PermissionRequest".into();
        assert_eq!(step(Some(Executing), sub), Some(Waiting));
    }

    #[test]
    fn resume_and_processes() {
        assert_eq!(OPENCODE.resume_args(Some("ses_1"), false), ["--session", "ses_1"]);
        assert_eq!(OPENCODE.resume_args(None, true), ["--continue"]);
        assert!(OPENCODE.resume_args(None, false).is_empty());
        assert_eq!(OPENCODE.program(), "opencode");
        let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(OPENCODE.matches_process("opencode", &argv(&["opencode"])));
        assert!(OPENCODE.matches_process("node", &argv(&["node", "/usr/lib/node_modules/opencode-ai/bin/opencode"])));
        assert!(!OPENCODE.matches_process("node", &argv(&["node", "server.js"])));
        assert!(matches!(OPENCODE.hook_install(), HookInstall::OwnedFile(OwnedFile { register: None, .. })));
    }

    #[test]
    fn sessions_are_read_from_opencodes_database() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("opencode.db");
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE session (id TEXT PRIMARY KEY, parent_id TEXT, directory TEXT NOT NULL, title TEXT,
                time_created INTEGER, time_updated INTEGER NOT NULL, time_archived INTEGER);
             INSERT INTO session VALUES ('ses_old', NULL, '/w/repo', 't', 0, 1000, NULL);
             INSERT INTO session VALUES ('ses_new', NULL, '/w/repo/pkg', 't', 0, 5000, NULL);
             INSERT INTO session VALUES ('ses_child', 'ses_new', '/w/repo', 't', 0, 6000, NULL);
             INSERT INTO session VALUES ('ses_gone', NULL, '/w/repo', 't', 0, 7000, 7000);
             INSERT INTO session VALUES ('ses_else', NULL, '/w/repo2', 't', 0, 8000, NULL);",
        )
        .unwrap();
        drop(conn);
        let found = sessions_in_db(&db, Path::new("/w/repo")).unwrap();
        let ids: Vec<&str> = found.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["ses_new", "ses_old"]);
        assert_eq!(found[0].cwd, "/w/repo/pkg");
        assert_eq!(found[0].modified, SystemTime::UNIX_EPOCH + Duration::from_millis(5000));
        assert_eq!(found[0].agent, HarnessKind::OPENCODE);
        assert!(sessions_in_db(&dir.path().join("missing.db"), Path::new("/w")).is_none());
        let other = dir.path().join("other.db");
        rusqlite::Connection::open(&other).unwrap().execute_batch("CREATE TABLE x (a)").unwrap();
        assert!(sessions_in_db(&other, Path::new("/w")).is_none());
    }
}
