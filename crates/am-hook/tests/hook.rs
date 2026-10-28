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

/// The payloads Pi's generated extension (`am_core::harness::pi`) sends:
/// Claude-shaped, Pi's tool names, `--worktree ""` (found from `cwd`).
#[test]
fn a_pi_session_is_tracked_through_its_extension_payloads() {
    let env = Env::new();
    let cwd = format!("{}/pkg", env.root());
    std::fs::create_dir_all(&cwd).unwrap();
    let pi = |event: &str, extra: Value| {
        let mut p = json!({ "session_id": "01a18110-925d", "cwd": cwd, "hook_event_name": event });
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        env.send_raw("pi", event, "", p);
    };
    pi("SessionStart", json!({}));
    assert_eq!((env.status().0.as_str(), env.agent().as_deref()), ("idle", Some("pi")));
    pi("UserPromptSubmit", json!({}));
    assert_eq!(env.status().0, "executing");
    pi("PreToolUse", json!({ "tool_name": "bash", "tool_input": { "command": "cargo test" } }));
    // A dialog during the turn (ctx.ui.confirm) waits on the user; its answer resumes.
    pi("PermissionRequest", json!({ "message": "Allow?" }));
    assert_eq!(env.status().0, "waiting");
    pi("PostToolUse", json!({}));
    assert_eq!(env.status().0, "executing");
    pi("PostToolUse", json!({ "tool_name": "bash" }));
    pi("SubagentStart", json!({ "agent_id": "call_2", "agent_type": "subagent", "tool_input": { "description": "scan" } }));
    assert_eq!(env.running_subagents_of("01a18110-925d"), 1);
    pi("SubagentStop", json!({ "agent_id": "call_2", "agent_type": "subagent" }));
    assert_eq!(env.running_subagents_of("01a18110-925d"), 0);
    pi("Stop", json!({}));
    assert_eq!(env.status().0, "done");
    assert_eq!(env.session("01a18110-925d"), ("done".to_string(), true));
    pi("SessionEnd", json!({}));
    assert!(!env.session("01a18110-925d").1);
}

#[test]
fn a_qoder_session_is_tracked_with_its_subagents() {
    let env = Env::new();
    let qoder = |event: &str, payload: Value| env.send_as(Some("qoder"), event, with_session("q-1", payload));
    qoder("SessionStart", json!({ "source": "startup", "model": "auto" }));
    assert_eq!((env.status().0.as_str(), env.agent().as_deref()), ("idle", Some("qoder")));
    qoder("UserPromptSubmit", json!({ "prompt": "fix it" }));
    assert_eq!(env.status().0, "executing");
    qoder("PermissionRequest", json!({ "tool_name": "Bash", "tool_input": { "command": "rm -rf target" } }));
    assert_eq!(env.status().0, "waiting");
    qoder("PostToolUse", json!({ "tool_name": "Bash", "tool_use_id": "t1" }));
    qoder("SubagentStart", sub("qa1", json!({})));
    qoder("Stop", json!({ "last_assistant_message": "Started a helper.", "stop_hook_active": false }));
    assert_eq!(env.status(), ("delegating".into(), 1));
    qoder("SubagentStop", sub("qa1", json!({ "last_assistant_message": "report" })));
    assert_eq!(env.status(), ("done".into(), 0));
    assert_eq!(env.session("q-1").0, "done");
}

#[test]
fn a_qoder_ide_turn_is_found_from_the_payload_cwd() {
    // The Qoder IDE runs the same hooks without `$QODER_PROJECT_DIR`, so the
    // hook passes `--worktree ""` and the payload `cwd` places the session.
    let env = Env::new();
    let cwd = format!("{}/pkg", env.root());
    std::fs::create_dir_all(&cwd).unwrap();
    let ide = |event: &str, extra: Value| {
        let mut p = json!({ "hook_event_name": event, "session_id": "ide-1", "cwd": cwd });
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        env.send_raw("qoder", event, "", p);
    };
    ide("UserPromptSubmit", json!({ "prompt": "fix it" }));
    assert_eq!((env.status().0.as_str(), env.agent().as_deref()), ("executing", Some("qoder")));
    ide("PermissionRequest", json!({ "tool_name": "Bash", "tool_input": { "command": "rm -rf target" } }));
    assert_eq!(env.status().0, "waiting");
    ide("PostToolUse", json!({ "tool_name": "Bash" }));
    ide("Stop", json!({ "stop_hook_active": false }));
    assert_eq!(env.status().0, "done");
    assert_eq!(env.session("ide-1").0, "done");
}

#[test]
fn a_codebuddy_turn_is_tracked() {
    let env = Env::new();
    let root = env.root();
    let transcript = format!("{root}/.codebuddy/projects/w/cb-1.jsonl");
    let cb = |event: &str, extra: Value| {
        let mut p = json!({ "hook_event_name": event, "session_id": "cb-1", "cwd": root,
                            "transcript_path": transcript, "permission_mode": "default" });
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        env.send_raw("codebuddy", event, &root, p);
    };
    cb("SessionStart", json!({ "source": "startup" }));
    assert_eq!((env.status().0.as_str(), env.agent().as_deref()), ("idle", Some("codebuddy")));
    cb("UserPromptSubmit", json!({ "prompt": "fix it" }));
    assert_eq!(env.status().0, "executing");
    cb("PreToolUse", json!({ "tool_name": "Bash", "tool_input": { "command": "cargo test" } }));
    cb("Notification", json!({ "notification_type": "permission_prompt", "message": "CodeBuddy needs your permission to use Bash" }));
    assert_eq!(env.status().0, "waiting");
    cb("PostToolUse", json!({ "tool_name": "Bash", "tool_response": {} }));
    assert_eq!(env.status().0, "executing");
    cb("Stop", json!({ "stop_hook_active": false }));
    assert_eq!(env.status().0, "done");
    assert_eq!(env.session("cb-1").0, "done");
}

/// CodeBuddy's subagent hooks carry `agent_id`/`agent_type` (its
/// `buildAgentTrigger`) with the parent's `session_id`, and `Stop` fires for
/// the main session only, so a turn that hands off is `delegating`.
#[test]
fn a_codebuddy_subagent_keeps_the_turn_delegating() {
    let env = Env::new();
    let root = env.root();
    let cb = |event: &str, extra: Value| {
        let mut p = json!({ "hook_event_name": event, "session_id": "cb-2", "cwd": root });
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        env.send_raw("codebuddy", event, &root, p);
    };
    let agent = json!({ "agent_id": "store-1", "agent_type": "general-purpose" });
    cb("UserPromptSubmit", json!({ "prompt": "split it up" }));
    cb("SubagentStart", agent.clone());
    let mut tool = agent.clone();
    tool.as_object_mut().unwrap().extend(json!({ "tool_name": "Read", "tool_input": { "file_path": "/w/a.rs" } }).as_object().unwrap().clone());
    cb("PreToolUse", tool);
    assert_eq!(env.status(), ("executing".into(), 1), "a subagent's tool doesn't move the parent");
    cb("Stop", json!({ "stop_hook_active": false }));
    assert_eq!(env.status(), ("delegating".into(), 1));
    cb("SubagentStop", agent);
    assert_eq!(env.status(), ("done".into(), 0));
}

#[test]
fn a_muse_session_is_tracked_and_its_child_sessions_are_ignored() {
    let env = Env::new();
    let cwd = format!("{}/pkg", env.root());
    std::fs::create_dir_all(&cwd).unwrap();
    // Muse has no project-dir variable (`--worktree ""`); `turn_id` marks a turn.
    let muse = |event: &str, sid: &str, turn: Option<&str>, extra: Value| {
        let mut p = json!({ "hook_event_name": event, "session_id": sid, "cwd": cwd });
        if let Some(t) = turn {
            p["turn_id"] = t.into();
        }
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        env.send_raw("muse", event, "", p);
    };
    muse("SessionStart", "m-1", None, json!({}));
    muse("UserPromptSubmit", "m-1", Some("t-1"), json!({ "prompt": "fix it" }));
    assert_eq!((env.status().0.as_str(), env.agent().as_deref()), ("executing", Some("muse")));
    // An internal child session (turn id = its own session id; lifecycle
    // events carry no turn id) adds nothing.
    muse("SessionStart", "c-1", None, json!({}));
    muse("UserPromptSubmit", "c-1", Some("c-1"), json!({}));
    muse("Stop", "c-1", Some("c-1"), json!({}));
    muse("SessionEnd", "c-1", None, json!({}));
    let sessions: i64 = env.db().query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0)).unwrap();
    assert_eq!(sessions, 1);
    assert_eq!(env.status().0, "executing");
    // ...unless it's blocked on the user: the owner reads waiting until the
    // top-level session's next event.
    muse("Notification", "c-2", Some("c-2"), json!({ "notification_type": "permission_prompt", "message": "Allow?" }));
    assert_eq!(env.status().0, "waiting");
    let sessions: i64 = env.db().query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0)).unwrap();
    assert_eq!(sessions, 1);
    muse("Stop", "m-1", Some("t-1"), json!({ "last_assistant_message": "Done." }));
    assert_eq!(env.session("m-1"), ("done".into(), true));
    assert_eq!(env.status().0, "done");
    // A permission prompt, then the turn ends: done (no tool hooks say it was answered).
    muse("UserPromptSubmit", "m-1", Some("t-2"), json!({}));
    muse("Notification", "m-1", Some("t-2"), json!({ "notification_type": "permission_prompt", "message": "Allow?" }));
    assert_eq!(env.status().0, "waiting");
    muse("Stop", "m-1", Some("t-2"), json!({}));
    assert_eq!((env.status().0.as_str(), env.session("m-1").0.as_str()), ("done", "done"));
    muse("SessionEnd", "m-1", None, json!({}));
    assert!(!env.session("m-1").1);
}

#[test]
fn a_kiro_session_is_tracked_through_its_agent_hooks() {
    let env = Env::new();
    let cwd = format!("{}/pkg", env.root());
    std::fs::create_dir_all(&cwd).unwrap();
    // Kiro has no project-dir variable: the hook passes `--worktree ""`.
    let kiro = |event: &str, extra: Value| {
        let mut p = json!({ "hook_event_name": event, "cwd": cwd, "session_id": "k-1" });
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        env.send_raw("kiro", event, "", p);
    };
    kiro("agentSpawn", json!({}));
    assert_eq!((env.status().0.as_str(), env.agent().as_deref()), ("idle", Some("kiro")));
    kiro("userPromptSubmit", json!({ "prompt": "fix the bug" }));
    assert_eq!(env.status().0, "executing");
    kiro("preToolUse", json!({ "tool_name": "execute_bash", "tool_input": { "command": "cargo test" } }));
    kiro("postToolUse", json!({ "tool_name": "execute_bash", "tool_response": { "success": true } }));
    assert_eq!(env.status().0, "executing");
    kiro("stop", json!({}));
    assert_eq!(env.status().0, "done");
    assert_eq!(env.session("k-1").0, "done");
}

#[test]
fn a_rovodev_session_without_session_ids_updates_the_worktree_row() {
    let env = Env::new();
    // The hook passes `--worktree "$PWD"`: where `acli rovodev run` started.
    let pwd = format!("{}/pkg", env.root());
    std::fs::create_dir_all(&pwd).unwrap();
    let rovo = |event: &str, payload: Value| env.send_raw("rovodev", event, &pwd, payload);
    rovo("on_session_start", json!({}));
    assert_eq!((env.status().0.as_str(), env.agent().as_deref()), ("idle", Some("rovodev")));
    rovo("on_user_prompt", json!({}));
    assert_eq!(env.status().0, "executing");
    let tool = json!({ "attributes": { "tool_input": { "tool_name": "bash", "args": { "command": "cargo test" } } } });
    rovo("on_tool_permission", tool.clone());
    assert_eq!(env.status().0, "waiting");
    rovo("on_tool_start", tool.clone());
    rovo("on_tool_end", tool);
    assert_eq!(env.status().0, "executing");
    rovo("on_complete", json!({ "transcript_path": "/tmp/gone.json" }));
    assert_eq!(env.status().0, "done");
    let sessions: i64 = env.db().query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0)).unwrap();
    assert_eq!(sessions, 0, "no session id, so no session rows");
    rovo("on_user_prompt", json!({}));
    rovo("on_error", json!({}));
    assert_eq!(env.status().0, "idle");
}

/// Current Rovo Dev (202608.25.1) sends `{session_id, transcript_path, cwd,
/// timestamp, hook_event_name, attributes}`: the session gets its own row,
/// headlines the worktree, and `on_session_end` closes it.
#[test]
fn a_rovodev_session_with_the_real_envelope_gets_a_session_row() {
    let env = Env::new();
    let pwd = format!("{}/pkg", env.root());
    std::fs::create_dir_all(&pwd).unwrap();
    let rovo = |event: &str, attributes: Value| {
        let payload = json!({ "session_id": "rs-1", "transcript_path": "/tmp/rs-1.json", "cwd": pwd, "timestamp": "2026-10-28T05:00:00Z",
                              "hook_event_name": event, "attributes": attributes });
        env.send_raw("rovodev", event, &pwd, payload);
    };
    rovo("on_session_start", json!({}));
    assert_eq!(env.session("rs-1"), ("idle".into(), true));
    rovo("on_user_prompt", json!({ "prompt": "fix it" }));
    assert_eq!((env.status().0.as_str(), env.agent().as_deref(), env.headline().as_str()), ("executing", Some("rovodev"), "rs-1"));
    rovo("on_tool_permission", json!({ "tool_name": "bash", "args": { "command": "cargo test" } }));
    assert_eq!(env.session("rs-1").0, "waiting");
    rovo("on_tool_start", json!({ "tool_calls": [{ "tool_name": "bash", "args": { "command": "cargo test" } }] }));
    assert_eq!(env.status().0, "executing");
    rovo("on_complete", json!({}));
    assert_eq!((env.status().0.as_str(), env.session("rs-1").0.as_str()), ("done", "done"));
    rovo("on_session_end", json!({}));
    assert!(!env.session("rs-1").1, "on_session_end closes the session");
}

#[test]
fn a_kimi_session_is_tracked_through_its_native_events() {
    let env = Env::new();
    let cwd = format!("{}/src", env.root());
    std::fs::create_dir_all(&cwd).unwrap();
    // Kimi has no project-dir variable either: `--worktree ""`, payload `cwd`.
    let kimi = |event: &str, extra: Value| {
        let mut p = json!({ "hook_event_name": event, "session_id": "k-1", "session_title": "Fix", "client_type": "kimi_code_cli", "cwd": cwd });
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        env.send_raw("kimi", event, "", p);
    };
    kimi("SessionStart", json!({ "source": "startup", "model": "kimi-k2", "profile": "default" }));
    assert_eq!((env.status().0.as_str(), env.agent().as_deref()), ("idle", Some("kimi")));
    kimi("UserPromptSubmit", json!({ "prompt": "fix it", "is_steer": false }));
    assert_eq!(env.status().0, "executing");
    kimi("PreToolUse", json!({ "tool_name": "Bash", "tool_input": { "command": "cargo test" }, "tool_call_id": "c1" }));
    kimi("PermissionRequest", json!({ "tool_name": "Bash" }));
    assert_eq!(env.status().0, "waiting");
    kimi("PermissionResult", json!({ "tool_name": "Bash" }));
    assert_eq!(env.status().0, "executing");
    kimi("PostToolUseFailure", json!({ "tool_name": "Bash", "tool_call_id": "c1", "error": { "message": "exit 1" } }));
    kimi("Stop", json!({ "stop_hook_active": false }));
    assert_eq!(env.status().0, "done");
    assert_eq!(env.session("k-1").0, "done");
    // Esc mid-turn fires Interrupt, not Stop.
    kimi("UserPromptSubmit", json!({ "prompt": "again" }));
    kimi("Interrupt", json!({ "turn_id": 2, "reason": "cancelled" }));
    assert_eq!(env.status().0, "idle");
    kimi("SessionEnd", json!({ "reason": "exit" }));
    assert_eq!(env.session("k-1"), ("idle".into(), false));
}

#[test]
fn a_hermes_session_is_tracked_through_its_shell_hooks() {
    let env = Env::new();
    let cwd = format!("{}/pkg", env.root());
    std::fs::create_dir_all(&cwd).unwrap();
    // Hermes's stdin payload; no project-dir variable, so `--worktree ""`.
    let hermes = |event: &str, session: &str, top: Value, extra: Value| {
        let mut p = json!({ "hook_event_name": event, "tool_name": null, "tool_input": null, "session_id": session,
                            "cwd": cwd, "profile": "default", "extra": extra });
        p.as_object_mut().unwrap().extend(top.as_object().unwrap().clone());
        env.send_raw("hermes", event, "", p);
    };
    let sid = "20261028_101500_ab12cd";
    hermes("on_session_start", sid, json!({}), json!({ "model": "anthropic/claude-sonnet", "platform": "cli" }));
    assert_eq!((env.status().0.as_str(), env.agent().as_deref()), ("idle", Some("hermes")));
    hermes("pre_llm_call", sid, json!({}), json!({ "user_message": "fix the bug", "is_first_turn": true }));
    assert_eq!(env.status().0, "executing");
    let turn = json!({ "turn_id": format!("{sid}:task-1:aa11") });
    hermes("pre_tool_call", sid, json!({ "tool_name": "terminal", "tool_input": { "command": "rm -rf target" } }), turn.clone());
    // A person is needed: approval, sudo password or clarify.
    hermes("on_human_input_request", sid, json!({}), json!({ "kind": "approval", "request_id": "r1", "session_key": "k", "platform": "cli" }));
    assert_eq!(env.status().0, "waiting");
    hermes("on_human_input_resolved", sid, json!({}), json!({ "kind": "approval", "request_id": "r1", "outcome": "approved", "platform": "cli" }));
    assert_eq!(env.status().0, "executing");
    hermes("post_tool_call", sid, json!({ "tool_name": "terminal" }), json!({ "status": "ok", "turn_id": turn["turn_id"] }));
    hermes("on_session_end", sid, json!({}), json!({ "completed": true, "interrupted": false, "turn_id": turn["turn_id"], "platform": "cli" }));
    assert_eq!(env.status().0, "done");
    assert_eq!(env.session(sid).0, "done");

    // The background memory/skill review fork after the turn: same session
    // id, its own turn, tool events only. The turn stays done.
    let fork = json!({ "turn_id": format!("{sid}:task-2:bb22") });
    hermes("pre_tool_call", sid, json!({ "tool_name": "memory", "tool_input": { "action": "add" } }), fork.clone());
    hermes("post_tool_call", sid, json!({ "tool_name": "memory" }), fork.clone());
    hermes("pre_tool_call", sid, json!({ "tool_name": "read_file", "tool_input": { "path": "x" } }), fork);
    assert_eq!(env.session(sid).0, "done");
    assert_eq!(env.status().0, "done");

    // A gateway (Telegram) turn in the same directory is left out entirely.
    let chat = "20261028_120000_cc33";
    hermes("pre_llm_call", chat, json!({}), json!({ "user_message": "hi", "platform": "telegram" }));
    hermes("pre_tool_call", chat, json!({ "tool_name": "terminal" }), json!({ "turn_id": "c" }));
    hermes("on_session_end", chat, json!({}), json!({ "completed": true, "platform": "telegram" }));
    let chats: i64 = env.db().query_row("SELECT COUNT(*) FROM sessions WHERE session_id = ?1", [chat], |r| r.get(0)).unwrap();
    assert_eq!(chats, 0);
    assert_eq!(env.status().0, "done");

    hermes("on_session_finalize", sid, json!({}), json!({ "platform": "cli" }));
    assert_eq!(env.session(sid), ("idle".into(), false));
}

#[test]
fn an_opencode_session_is_tracked_through_its_plugins_payloads() {
    let env = Env::new();
    let cwd = format!("{}/pkg", env.root());
    std::fs::create_dir_all(&cwd).unwrap();
    // Exactly what am's OpenCode plugin sends: `--worktree ""`, canonical
    // event names, a child session reported as a subagent of its parent.
    let opencode = |event: &str, extra: Value| {
        let mut p = json!({ "session_id": "ses_1", "cwd": cwd, "hook_event_name": event });
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        env.send_raw("opencode", event, "", p);
    };
    opencode("SessionStart", json!({}));
    assert_eq!((env.status().0.as_str(), env.agent().as_deref()), ("idle", Some("opencode")));
    opencode("UserPromptSubmit", json!({}));
    assert_eq!(env.status().0, "executing");
    opencode("PreToolUse", json!({ "tool_name": "bash", "tool_input": { "command": "cargo test" } }));
    opencode("PermissionRequest", json!({ "tool_name": "bash" }));
    assert_eq!(env.status().0, "waiting");
    opencode("PostToolUse", json!({}));
    assert_eq!(env.status().0, "executing");

    let child = json!({ "agent_id": "ses_2", "agent_type": "explore" });
    opencode("SubagentStart", child.clone());
    assert_eq!(env.status(), ("executing".into(), 1));
    let mut read = child.clone();
    read.as_object_mut().unwrap().extend(json!({ "tool_name": "read", "tool_input": { "file_path": "src/a.rs" } }).as_object().unwrap().clone());
    opencode("PreToolUse", read);
    let (tool, input): (String, String) = env
        .db()
        .query_row("SELECT last_tool, last_tool_input FROM subagents WHERE agent_id = 'ses_2'", [], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    assert_eq!((tool.as_str(), input.as_str()), ("Read", "src/a.rs"));
    let mut ask = child.clone();
    ask["tool_name"] = "edit".into();
    opencode("PermissionRequest", ask);
    assert_eq!(env.status().0, "waiting");
    opencode("PostToolUse", json!({}));
    opencode("SubagentStop", child);
    assert_eq!(env.status(), ("executing".into(), 0));

    opencode("Stop", json!({}));
    assert_eq!(env.status().0, "done");
    assert_eq!(env.session("ses_1").0, "done");
    // The child never became a session of its own.
    let sessions: i64 = env.db().query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0)).unwrap();
    assert_eq!(sessions, 1);

    opencode("UserPromptSubmit", json!({}));
    opencode("StopFailure", json!({}));
    assert_eq!(env.status().0, "idle");
    opencode("SessionEnd", json!({}));
    assert!(!env.session("ses_1").1);
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
fn an_antigravity_conversation_is_tracked_from_its_payload() {
    let env = Env::new();
    let root = env.root();
    let agy = |event: &str, extra: Value| {
        let mut p = json!({ "conversationId": "ec33ebf9", "workspacePaths": [root],
                            "transcriptPath": "/h/.gemini/antigravity-cli/brain/ec33ebf9/t.jsonl", "modelName": "gemini" });
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        // No project-dir variable: the command passes --worktree "".
        env.send_raw("antigravity", event, "", p);
    };
    agy("SessionStart", json!({}));
    agy("PreInvocation", json!({ "invocationNum": 0, "initialNumSteps": 1 }));
    assert_eq!((env.status().0.as_str(), env.agent().as_deref()), ("executing", Some("antigravity")));
    agy("PreInvocation", json!({ "invocationNum": 1, "initialNumSteps": 3 }));
    agy("Stop", json!({ "executionNum": 1, "terminationReason": "model_stop", "fullyIdle": false }));
    assert_eq!(env.status().0, "executing", "background work still running");
    agy("Stop", json!({ "executionNum": 1, "terminationReason": "model_stop", "fullyIdle": true }));
    assert_eq!(env.status().0, "done");
    assert_eq!(env.session("ec33ebf9").0, "done");
}

#[test]
fn a_new_antigravity_conversation_closes_the_settled_ones() {
    // Antigravity has no session-end hook, so a new conversation closes the
    // owner's done/idle ones; a still-running one stays open.
    let env = Env::new();
    let root = env.root();
    let agy = |sid: &str, event: &str, extra: Value| {
        let mut p = json!({ "conversationId": sid, "workspacePaths": [root] });
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        env.send_raw("antigravity", event, "", p);
    };
    agy("old", "SessionStart", json!({}));
    agy("old", "PreInvocation", json!({ "invocationNum": 0 }));
    agy("old", "Stop", json!({ "fullyIdle": true }));
    agy("busy", "SessionStart", json!({}));
    agy("busy", "PreInvocation", json!({ "invocationNum": 0 }));
    assert_eq!(env.session("old"), ("done".into(), false), "closed by busy's start");
    agy("new", "SessionStart", json!({}));
    assert_eq!(env.session("busy"), ("executing".into(), true), "a running conversation is left open");
    assert_eq!(env.session("new"), ("idle".into(), true));
    // A Claude session in the same worktree is another harness's: untouched.
    env.send("SessionStart", with_session("c", json!({ "source": "startup" })));
    env.send("Stop", with_session("c", json!({})));
    agy("newer", "SessionStart", json!({}));
    assert!(env.session("c").1, "only Antigravity's own sessions close");
    assert!(!env.session("new").1);
    // A closed conversation that is resumed opens again.
    agy("old", "PreInvocation", json!({ "invocationNum": 0 }));
    assert_eq!(env.session("old"), ("executing".into(), true));
}

#[test]
fn a_grok_session_is_tracked_from_camel_case_payloads() {
    let env = Env::new();
    let cwd = format!("{}/pkg", env.root());
    std::fs::create_dir_all(&cwd).unwrap();
    // Grok strips the hook's env: `--worktree ""` and the payload's `cwd`.
    let grok = |event: &str, extra: Value| {
        let mut p = json!({ "hookEventName": event, "sessionId": "g-1", "cwd": cwd });
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        env.send_raw("grok", event, "", p);
    };
    grok("SessionStart", json!({}));
    grok("UserPromptSubmit", json!({ "prompt": "fix it" }));
    assert_eq!((env.status().0.as_str(), env.agent().as_deref()), ("executing", Some("grok")));
    grok("PreToolUse", json!({ "toolName": "bash", "toolInput": { "command": "ls" } }));
    // Sent for every tool step, even when nothing waits on the user.
    grok("Notification", json!({ "notificationType": "permission_prompt", "message": "Tool permission requested" }));
    assert_eq!(env.status().0, "executing");
    grok("Notification", json!({ "notificationType": "permission_prompt", "message": "Grok needs permission to run rm" }));
    assert_eq!(env.status().0, "waiting");
    grok("PreToolUse", json!({ "toolName": "bash" }));
    grok("Notification", json!({ "message": "Turn complete in 1.2s." }));
    grok("Stop", json!({}));
    assert_eq!(env.status().0, "done");
    assert_eq!(env.session("g-1"), ("done".into(), true));

    // A specific prompt the user answers, then the turn ends with no
    // `PreToolUse` in between: `Stop` still takes it to done.
    grok("UserPromptSubmit", json!({ "prompt": "clean up" }));
    grok("PreToolUse", json!({ "toolName": "bash", "toolInput": { "command": "rm -r build" } }));
    grok("Notification", json!({ "notificationType": "permission_prompt", "message": "Grok needs permission to run rm" }));
    assert_eq!(env.status().0, "waiting");
    grok("Stop", json!({}));
    assert_eq!(env.status().0, "done");
    assert_eq!(env.session("g-1"), ("done".into(), true));
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
