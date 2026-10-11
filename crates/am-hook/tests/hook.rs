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
    fn send(&self, event: &str, mut payload: Value) {
        payload["hook_event_name"] = event.into();
        payload["session_id"] = payload.get("session_id").cloned().unwrap_or("s1".into());
        let sub = self.worktree.path().join("pkg");
        std::fs::create_dir_all(&sub).unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_am-hook"))
            .args(["hook-event", "--worktree", sub.to_str().unwrap(), "--event", event])
            .env("AM_DATA_DIR", self.data.path())
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(payload.to_string().as_bytes()).unwrap();
        assert!(child.wait().unwrap().success());
    }

    fn status(&self) -> (String, i64) {
        self.db()
            .query_row("SELECT status, active_subagents FROM agent_status WHERE worktree_id = 'w'", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap()
    }
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
