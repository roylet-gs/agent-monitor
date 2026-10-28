//! End-to-end: run the real binary against an isolated AM_DATA_DIR and replay
//! the event sequence that broke the TS counter (taken from debug.log).

// Seeding DBs in this process must not write the real debug.log (the `am` children log to their temp dir).
am_core::silence_unit_tests!();

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use rusqlite::Connection;
use serde_json::{Value, json};

struct Env {
    data: tempfile::TempDir,
    worktree: tempfile::TempDir,
}

impl Env {
    fn new() -> Env {
        let data = tempfile::tempdir().unwrap();
        let worktree = tempfile::tempdir().unwrap();
        std::fs::write(worktree.path().join(".git"), "gitdir: /elsewhere\n").unwrap();
        let env = Env { data, worktree };
        let conn = env.db();
        conn.execute("INSERT INTO repositories (id, path, name) VALUES ('r', '/repo', 'repo')", []).unwrap();
        conn.execute(
            "INSERT INTO worktrees (id, repo_id, path, branch, name) VALUES ('w', 'r', ?1, 'feat', 'feat')",
            [env.root()],
        )
        .unwrap();
        env
    }

    fn root(&self) -> String {
        self.worktree.path().canonicalize().unwrap().to_string_lossy().into_owned()
    }

    fn db(&self) -> Connection {
        am_core_open(&self.data.path().join("agent-monitor.db"))
    }

    /// Hook from a subdirectory, like $CLAUDE_PROJECT_DIR pointing inside a monorepo.
    fn send(&self, event: &str, payload: Value) {
        self.send_as(None, event, payload);
    }

    /// `agent` is the `--agent` flag (`None` = a Claude hook, which has none).
    fn send_as(&self, agent: Option<&str>, event: &str, mut payload: Value) {
        payload["hook_event_name"] = event.into();
        payload["session_id"] = payload.get("session_id").cloned().unwrap_or("s1".into());
        let sub = self.worktree.path().join("pkg");
        std::fs::create_dir_all(&sub).unwrap();
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_am-hook"));
        cmd.arg("hook-event");
        if let Some(a) = agent {
            cmd.args(["--agent", a]);
        }
        let mut child = cmd
            .args(["--worktree", sub.to_str().unwrap(), "--event", event])
            .env("AM_DATA_DIR", self.data.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(payload.to_string().as_bytes()).unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success());
        // Gemini shows any hook output to the user, so the hook must be silent.
        assert!(out.stdout.is_empty() && out.stderr.is_empty(), "hook printed: {out:?}");
    }

    /// A hook exactly as a harness sends it: no added keys, any `--worktree`.
    fn send_raw(&self, agent: &str, event: &str, worktree: &str, payload: Value) {
        let mut child = Command::new(env!("CARGO_BIN_EXE_am-hook"))
            .args(["hook-event", "--agent", agent, "--worktree", worktree, "--event", event])
            .env("AM_DATA_DIR", self.data.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(payload.to_string().as_bytes()).unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success() && out.stdout.is_empty() && out.stderr.is_empty(), "hook printed: {out:?}");
    }

    /// Status rows written so far (0 before the binary first migrated the DB).
    fn status_rows(&self) -> i64 {
        let db = self.db();
        let exists: i64 = db.query_row("SELECT COUNT(*) FROM sqlite_master WHERE name = 'agent_status'", [], |r| r.get(0)).unwrap();
        if exists == 0 { 0 } else { db.query_row("SELECT COUNT(*) FROM agent_status", [], |r| r.get(0)).unwrap() }
    }

    fn agent(&self) -> Option<String> {
        self.db().query_row("SELECT agent FROM agent_status WHERE worktree_id = 'w'", [], |r| r.get(0)).unwrap()
    }

    fn status(&self) -> (String, i64) {
        self.db()
            .query_row("SELECT status, active_subagents FROM agent_status WHERE worktree_id = 'w'", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap()
    }
}

impl Env {
    /// (status, is_open) of one session.
    fn session(&self, sid: &str) -> (String, bool) {
        self.db()
            .query_row("SELECT status, is_open FROM sessions WHERE session_id = ?1", [sid], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
    }

    /// Headline session id on the worktree's `agent_status` row.
    fn headline(&self) -> String {
        self.db().query_row("SELECT session_id FROM agent_status WHERE worktree_id = 'w'", [], |r| r.get(0)).unwrap()
    }

    fn running_subagents_of(&self, sid: &str) -> i64 {
        self.db()
            .query_row("SELECT COUNT(*) FROM subagents WHERE session_id = ?1 AND status = 'running'", [sid], |r| r.get(0))
            .unwrap()
    }
}

fn with_session(sid: &str, mut v: Value) -> Value {
    v["session_id"] = sid.into();
    v
}

fn am_core_open(p: &Path) -> Connection {
    let c = Connection::open(p).unwrap();
    // Let the binary own migrations; tests just need the tables to exist.
    c.execute_batch(
        "PRAGMA journal_mode=WAL;
         CREATE TABLE IF NOT EXISTS repositories (id TEXT PRIMARY KEY, path TEXT NOT NULL UNIQUE, name TEXT NOT NULL, last_used_at TEXT NOT NULL DEFAULT (datetime('now')));
         CREATE TABLE IF NOT EXISTS worktrees (id TEXT PRIMARY KEY, repo_id TEXT NOT NULL, path TEXT NOT NULL, branch TEXT NOT NULL, name TEXT NOT NULL, custom_name TEXT, created_at TEXT NOT NULL DEFAULT (datetime('now')), UNIQUE(repo_id, branch));",
    )
    .unwrap();
    c
}

fn sub(agent: &str, extra: Value) -> Value {
    let mut v = json!({ "agent_id": agent, "agent_type": "general-purpose" });
    if let Value::Object(m) = extra {
        for (k, x) in m {
            v[k] = x;
        }
    }
    v
}

#[test]
fn background_subagents_keep_worktree_delegating_until_the_last_finishes() {
    let env = Env::new();
    env.send("UserPromptSubmit", json!({ "permission_mode": "default" }));
    assert_eq!(env.status().0, "executing");

    for a in ["a1", "a2", "a3", "a4"] {
        env.send("SubagentStart", sub(a, json!({})));
    }
    env.send("Stop", json!({}));
    assert_eq!(env.status(), ("delegating".into(), 4));

    // The regression: subagent tool calls used to flip this back to executing.
    env.send("PreToolUse", sub("a2", json!({ "tool_name": "Bash", "tool_input": { "command": "grep WARN debug.log" } })));
    env.send("PostToolUse", sub("a2", json!({ "tool_name": "Bash" })));
    assert_eq!(env.status(), ("delegating".into(), 4));

    // Internal agents (empty agent_type) and duplicate stops must not drain the count.
    env.send("SubagentStop", json!({ "agent_id": "internal", "agent_type": "" }));
    env.send("SubagentStop", sub("a1", json!({})));
    env.send("SubagentStop", sub("a1", json!({})));
    assert_eq!(env.status(), ("delegating".into(), 3));

    for a in ["a2", "a3"] {
        env.send("SubagentStop", sub(a, json!({})));
    }
    assert_eq!(env.status(), ("delegating".into(), 1));
    env.send("SubagentStop", sub("a4", json!({ "last_assistant_message": "report" })));
    assert_eq!(env.status(), ("done".into(), 0));

    let (tools, last_tool, input): (i64, String, String) = env
        .db()
        .query_row("SELECT tool_count, last_tool, last_tool_input FROM subagents WHERE agent_id = 'a2'", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap();
    assert_eq!((tools, last_tool.as_str(), input.as_str()), (1, "Bash", "grep WARN debug.log"));
    let internal: i64 = env.db().query_row("SELECT COUNT(*) FROM subagents WHERE agent_id = 'internal'", [], |r| r.get(0)).unwrap();
    assert_eq!(internal, 0);
}

#[test]
fn a_gemini_session_is_tracked_through_its_native_events() {
    let env = Env::new();
    let gemini = |event: &str, payload: Value| env.send_as(Some("gemini"), event, payload);
    gemini("SessionStart", json!({ "source": "startup", "transcript_path": "" }));
    assert_eq!((env.status().0.as_str(), env.agent().as_deref()), ("idle", Some("gemini")));
    gemini("BeforeAgent", json!({ "prompt": "fix the bug" }));
    assert_eq!(env.status().0, "executing");
    gemini("Notification", json!({ "notification_type": "ToolPermission", "message": "Allow replace?", "details": {} }));
    assert_eq!(env.status().0, "waiting");
    gemini("BeforeTool", json!({ "tool_name": "replace", "tool_input": { "file_path": "src/a.rs" } }));
    gemini("BeforeTool", json!({ "tool_name": "invoke_agent", "tool_input": { "agent_name": "codebase_investigator" } }));
    gemini("AfterTool", json!({ "tool_name": "invoke_agent" }));
    assert_eq!(env.status(), ("executing".into(), 0));
    gemini("AfterAgent", json!({ "prompt": "fix the bug", "prompt_response": "Fixed it.", "stop_hook_active": false }));
    assert_eq!(env.status().0, "done");
    let (last, session): (String, String) = env
        .db()
        .query_row("SELECT last_response, session_id FROM agent_status WHERE worktree_id = 'w'", [], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    assert_eq!((last.as_str(), session.as_str()), ("Fixed it.", "s1"));

    // A later Claude session in the same worktree takes the row over.
    env.send("UserPromptSubmit", json!({ "session_id": "c1" }));
    assert_eq!((env.status().0.as_str(), env.agent().as_deref()), ("executing", Some("claude")));
}

#[test]
fn a_manifest_plugin_is_tracked_with_no_code() {
    let env = Env::new();
    std::fs::create_dir_all(env.data.path().join("agents")).unwrap();
    std::fs::write(
        env.data.path().join("agents/mycli.json"),
        json!({
            "id": "mycli", "name": "My CLI", "program": "mycli",
            "hooks": {"settingsPath": "~/.mycli/hooks.json", "projectDirVar": "MYCLI_DIR"},
            "events": {"Prompt": "UserPromptSubmit", "ToolStart": "PreToolUse", "Done": "Stop"},
            "fields": {"session_id": "conversation", "last_assistant_message": "reply"}
        })
        .to_string(),
    )
    .unwrap();
    let mycli = |event: &str, payload: Value| env.send_as(Some("mycli"), event, payload);
    mycli("Prompt", json!({ "conversation": "m-1" }));
    assert_eq!((env.status().0.as_str(), env.agent().as_deref()), ("executing", Some("mycli")));
    mycli("ToolStart", json!({ "tool_name": "shell" }));
    mycli("Done", json!({ "reply": "All set." }));
    assert_eq!(env.status().0, "done");
    let (last, session): (String, String) = env
        .db()
        .query_row("SELECT last_response, session_id FROM agent_status WHERE worktree_id = 'w'", [], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    assert_eq!((last.as_str(), session.as_str()), ("All set.", "m-1"));
}

#[test]
fn a_codex_session_is_found_from_the_payload_cwd() {
    let env = Env::new();
    let cwd = format!("{}/pkg", env.root());
    std::fs::create_dir_all(&cwd).unwrap();
    // Codex has no project-dir variable: the hook passes `--worktree ""`.
    let codex = |event: &str, extra: Value| {
        let mut p = json!({ "hook_event_name": event, "session_id": "t-1", "cwd": cwd, "model": "gpt-5" });
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        env.send_raw("codex", event, "", p);
    };
    codex("UserPromptSubmit", json!({ "prompt": "fix it" }));
    assert_eq!((env.status().0.as_str(), env.agent().as_deref()), ("executing", Some("codex")));
    codex("PermissionRequest", json!({ "tool_name": "Bash", "tool_input": { "command": "rm -rf target" } }));
    assert_eq!(env.status().0, "waiting");
    codex("PostToolUse", json!({ "tool_name": "Bash" }));
    codex("Stop", json!({ "last_assistant_message": "Done.", "stop_hook_active": false }));
    assert_eq!(env.status().0, "done");
    assert_eq!(env.session("t-1").0, "done");
}

#[test]
fn an_amp_thread_is_tracked_through_its_plugins_payloads() {
    let env = Env::new();
    let cwd = format!("{}/pkg", env.root());
    std::fs::create_dir_all(&cwd).unwrap();
    // Exactly what am's Amp plugin sends: `--worktree ""`, canonical event
    // names, the thread id as the session.
    let amp = |event: &str, extra: Value| {
        let mut p = json!({ "session_id": "T-1", "cwd": cwd, "hook_event_name": event });
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        env.send_raw("amp", event, "", p);
    };
    amp("SessionStart", json!({}));
    assert_eq!((env.status().0.as_str(), env.agent().as_deref()), ("idle", Some("amp")));
    amp("UserPromptSubmit", json!({}));
    assert_eq!(env.status().0, "executing");
    amp("PreToolUse", json!({ "tool_name": "Bash", "tool_input": { "command": "cargo test" } }));
    amp("PermissionRequest", json!({ "tool_name": "Bash" }));
    assert_eq!(env.status().0, "waiting");
    amp("PostToolUse", json!({}));
    assert_eq!(env.status().0, "executing");
    amp("PreToolUse", json!({ "tool_name": "edit_file", "tool_input": { "file_path": "src/a.rs" } }));
    amp("PostToolUse", json!({ "tool_name": "edit_file" }));
    amp("Stop", json!({}));
    assert_eq!(env.status().0, "done");
    assert_eq!(env.session("T-1").0, "done");

    amp("UserPromptSubmit", json!({}));
    amp("PermissionRequest", json!({}));
    amp("PostToolUse", json!({}));
    amp("StopFailure", json!({}));
    assert_eq!(env.status().0, "idle");
    let sessions: i64 = env.db().query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0)).unwrap();
    assert_eq!(sessions, 1);
}

#[test]
fn a_cursor_session_is_tracked_and_its_claude_hooks_are_ignored() {
    let env = Env::new();
    let root = env.root();
    let payload = |event: &str| {
        json!({ "hook_event_name": event, "conversation_id": "conv-1", "generation_id": "g1",
                "cursor_version": "2.4.0", "workspace_roots": [root], "model": "gpt-5" })
    };
    // Cursor firing the user's Claude Code hooks: not a Claude session.
    env.send_raw("claude", "UserPromptSubmit", &root, payload("UserPromptSubmit"));
    assert_eq!(env.status_rows(), 0);

    env.send_raw("cursor", "beforeSubmitPrompt", &root, payload("beforeSubmitPrompt"));
    assert_eq!((env.status().0.as_str(), env.agent().as_deref()), ("executing", Some("cursor")));
    env.send_raw("cursor", "stop", &root, payload("stop"));
    assert_eq!(env.status().0, "done");
    assert_eq!(env.session("conv-1").0, "done");
}

#[test]
fn main_agent_activity_while_delegating_is_executing() {
    let env = Env::new();
    env.send("UserPromptSubmit", json!({}));
    env.send("SubagentStart", sub("a1", json!({})));
    env.send("Stop", json!({}));
    assert_eq!(env.status().0, "delegating");
    env.send("PreToolUse", json!({ "tool_name": "Read" }));
    assert_eq!(env.status().0, "executing");
}

#[test]
fn subagent_description_is_read_from_meta_json() {
    let env = Env::new();
    let proj = tempfile::tempdir().unwrap();
    let main = proj.path().join("s1.jsonl");
    let subdir = proj.path().join("s1").join("subagents");
    std::fs::create_dir_all(&subdir).unwrap();
    std::fs::write(subdir.join("agent-a1.jsonl"), "").unwrap();
    std::fs::write(subdir.join("agent-a1.meta.json"), r#"{"agentType":"Explore","description":"Find poll loops"}"#).unwrap();
    env.send("SubagentStart", sub("a1", json!({ "transcript_path": main.to_str().unwrap() })));
    let desc: String = env.db().query_row("SELECT description FROM subagents WHERE agent_id = 'a1'", [], |r| r.get(0)).unwrap();
    assert_eq!(desc, "Find poll loops");
}

#[test]
fn unknown_path_becomes_a_standalone_session() {
    let env = Env::new();
    let elsewhere = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_am-hook"))
        .args(["hook-event", "--worktree", elsewhere.path().to_str().unwrap(), "--event", "UserPromptSubmit"])
        .env("AM_DATA_DIR", env.data.path())
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(br#"{"session_id":"x"}"#).unwrap();
    assert!(child.wait().unwrap().success());
    let status: String = env.db().query_row("SELECT status FROM standalone_sessions", [], |r| r.get(0)).unwrap();
    assert_eq!(status, "executing");
}

#[test]
fn a_session_follows_its_directory_onto_a_worktree_added_mid_session() {
    let env = Env::new();
    // The repo isn't tracked yet: the session starts standalone.
    env.db().execute("DELETE FROM worktrees", []).unwrap();
    env.send("UserPromptSubmit", json!({}));
    env.send("PreToolUse", json!({ "tool_name": "Bash", "tool_input": { "command": "ls" } }));
    assert_eq!(env.status_rows(), 0);

    // The repo is added (raw insert, so the standalone row isn't promoted), then
    // an event that doesn't change the session's status still moves it.
    env.db()
        .execute("INSERT INTO worktrees (id, repo_id, path, branch, name) VALUES ('w', 'r', ?1, 'feat', 'feat')", [env.root()])
        .unwrap();
    env.send("PostToolUse", json!({ "tool_name": "Bash" }));
    assert_eq!(env.status(), ("executing".into(), 0));
    assert_eq!(env.headline(), "s1");
}

#[test]
fn garbage_input_never_fails_the_hook() {
    let env = Env::new();
    let mut child = Command::new(env!("CARGO_BIN_EXE_am-hook"))
        .args(["hook-event", "--worktree", &env.root(), "--event", "Stop"])
        .env("AM_DATA_DIR", env.data.path())
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"{not json").unwrap();
    assert!(child.wait().unwrap().success());
}

#[test]
fn two_main_sessions_in_one_worktree_keep_their_own_status() {
    let env = Env::new();
    let a = |e: &str, v: Value| env.send(e, with_session("A", v));
    let b = |e: &str, v: Value| env.send(e, with_session("B", v));

    a("SessionStart", json!({ "source": "startup" }));
    a("UserPromptSubmit", json!({}));
    a("PreToolUse", json!({ "tool_name": "Bash", "tool_input": { "command": "cargo test" } }));
    b("SessionStart", json!({ "source": "startup" }));
    b("UserPromptSubmit", json!({}));
    b("Stop", json!({ "last_assistant_message": "B is finished" }));

    // B finishing doesn't mark A's still-running turn as done.
    assert_eq!(env.session("A"), ("executing".into(), true));
    assert_eq!(env.session("B"), ("done".into(), true));
    assert_eq!(env.status().0, "executing", "headline = the most urgent session");
    assert_eq!(env.headline(), "A");

    // A permission prompt in B outranks A executing.
    b("UserPromptSubmit", json!({}));
    b("Notification", json!({ "notification_type": "permission_prompt", "message": "Allow Bash?" }));
    assert_eq!(env.session("B").0, "waiting");
    a("PreToolUse", json!({ "tool_name": "Read", "tool_input": { "file_path": "/x" } }));
    assert_eq!(env.status().0, "waiting", "A's tool call must not hide B waiting on the user");
    assert_eq!(env.headline(), "B");

    // B ends: A is the headline again, and the row stays open.
    b("SessionEnd", json!({ "reason": "prompt_input_exit" }));
    assert!(!env.session("B").1);
    assert_eq!(env.headline(), "A");
    let open: bool = env.db().query_row("SELECT is_open FROM agent_status WHERE worktree_id = 'w'", [], |r| r.get(0)).unwrap();
    assert!(open);
}

#[test]
fn a_new_session_keeps_another_live_sessions_subagents() {
    let env = Env::new();
    env.send("SessionStart", with_session("A", json!({ "source": "startup" })));
    env.send("UserPromptSubmit", with_session("A", json!({})));
    env.send("SubagentStart", with_session("A", sub("a1", json!({}))));
    env.send("Stop", with_session("A", json!({})));
    assert_eq!(env.session("A").0, "delegating");

    // A second session starting in the same worktree used to close a1.
    env.send("SessionStart", with_session("B", json!({ "source": "startup" })));
    env.send("UserPromptSubmit", with_session("B", json!({})));
    env.send("Stop", with_session("B", json!({})));
    assert_eq!(env.running_subagents_of("A"), 1);
    assert_eq!(env.session("A").0, "delegating", "B's subagent count (0) doesn't decide A's status");
    assert_eq!(env.session("B").0, "done");
    assert_eq!(env.status(), ("delegating".into(), 1));

    // A session that has ended does lose its leftovers on the next start.
    env.send("SessionEnd", with_session("A", json!({})));
    env.send("SessionStart", with_session("C", json!({ "source": "startup" })));
    assert_eq!(env.running_subagents_of("A"), 0);
}
