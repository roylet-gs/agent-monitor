//! End-to-end: run the real binary against an isolated AM_DATA_DIR and replay
//! the event sequence that broke the TS counter (taken from debug.log).

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

fn issues(env: &Env, plugin: &str) -> Vec<(String, String, i64)> {
    let conn = env.db();
    let mut stmt = conn.prepare("SELECT kind, key, count FROM plugin_issues WHERE plugin = ?1 ORDER BY kind, key").unwrap();
    stmt.query_map([plugin], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().map(Result::unwrap).collect()
}

fn captured_events(env: &Env, plugin: &str) -> Vec<String> {
    let dir = env.data.path().join("hook-captures").join(plugin);
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    names.sort();
    names.iter().map(|n| n.trim_end_matches(".json").rsplit('-').next().unwrap().to_owned()).collect()
}

#[test]
fn a_draft_plugin_captures_every_event_and_records_issues() {
    let env = Env::new();
    std::fs::create_dir_all(env.data.path().join("agents")).unwrap();
    std::fs::write(
        env.data.path().join("agents/toycli.json"),
        json!({
            "id": "toycli", "name": "Toy CLI", "program": "toycli", "status": "draft",
            "hooks": {"settingsPath": "~/.toycli/hooks.json", "projectDirVar": "TOY_DIR"},
            "events": {"Prompt": "UserPromptSubmit", "ToolStart": "PreToolUse", "Done": "Stop"},
            "tools": {"shell": "Bash"},
            "training": {"captureEvents": ["Thinking"]}
        })
        .to_string(),
    )
    .unwrap();
    let toy = |event: &str, payload: Value| env.send_as(Some("toycli"), event, payload);
    toy("Prompt", json!({ "cwd": "/w" }));
    assert_eq!(env.status().0, "executing");
    // A capture-only event is recorded but never moves the status (it would read as idle).
    toy("Thinking", json!({ "cwd": "/w" }));
    assert_eq!(env.status().0, "executing");
    toy("ToolStart", json!({ "cwd": "/w", "tool_name": "grep_files" }));
    toy("ToolStart", json!({ "cwd": "/w", "tool_name": "grep_files" }));
    toy("Done", json!({}));
    assert_eq!(env.status().0, "done");

    assert_eq!(captured_events(&env, "toycli"), ["Prompt", "Thinking", "ToolStart", "ToolStart", "Done"]);
    let capture: Value = {
        let dir = env.data.path().join("hook-captures/toycli");
        let first = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).min().unwrap();
        serde_json::from_str(&std::fs::read_to_string(first).unwrap()).unwrap()
    };
    assert_eq!(capture["nativeEvent"], "Prompt");
    assert_eq!(capture["payload"]["session_id"], "s1");
    assert!(capture["env"]["names"].as_array().unwrap().iter().any(|n| n == "AM_DATA_DIR"));
    assert!(capture["env"]["dirs"].get("AM_DATA_DIR").is_some(), "directory values are kept");
    assert_eq!(
        issues(&env, "toycli"),
        [
            ("missing_field".into(), "cwd".into(), 1),
            ("unmapped_event".into(), "Thinking".into(), 1),
            ("unmapped_tool".into(), "grep_files".into(), 2),
        ]
    );
}

#[test]
fn a_published_plugin_records_issues_but_captures_nothing() {
    let env = Env::new();
    std::fs::create_dir_all(env.data.path().join("agents")).unwrap();
    std::fs::write(
        env.data.path().join("agents/toycli.json"),
        json!({
            "id": "toycli", "name": "Toy CLI", "program": "toycli",
            "hooks": {"settingsPath": "~/.toycli/hooks.json", "projectDirVar": "TOY_DIR"},
            "events": {"Prompt": "UserPromptSubmit"}
        })
        .to_string(),
    )
    .unwrap();
    env.send_as(Some("toycli"), "Prompt", json!({ "cwd": "/w", "tool_name": "zap" }));
    assert_eq!(env.status().0, "executing");
    assert!(captured_events(&env, "toycli").is_empty());
    assert_eq!(issues(&env, "toycli"), [("unmapped_tool".into(), "zap".into(), 1)]);
}

#[test]
fn an_unknown_plugin_is_captured_and_never_treated_as_claude() {
    let env = Env::new();
    env.send_as(None, "UserPromptSubmit", json!({}));
    assert_eq!(env.status().0, "executing");
    // A draft whose manifest no longer loads: Claude's rules would read "Stop" as done.
    std::fs::create_dir_all(env.data.path().join("agents")).unwrap();
    std::fs::write(env.data.path().join("agents/broken.json"), "{ nope").unwrap();
    env.send_as(Some("broken"), "Stop", json!({}));
    assert_eq!(env.status().0, "executing", "status untouched");
    assert_eq!(env.agent().as_deref(), Some("claude"));
    assert_eq!(captured_events(&env, "broken"), ["Stop"]);
    assert_eq!(issues(&env, "broken"), [("unknown_plugin".into(), "broken".into(), 1)]);
    // Ids that aren't valid can't name a capture dir.
    env.send_as(Some("../evil"), "Stop", json!({}));
    assert!(!env.data.path().join("hook-captures/../evil").exists());
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
