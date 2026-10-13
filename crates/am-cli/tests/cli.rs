//! Integration tests for `am`: run the real binary against an isolated
//! `AM_DATA_DIR` and a temp `HOME` (never the real ~/.agent-monitor or
//! ~/.claude), with fake `gh`/editor binaries first on `PATH`. Ported from
//! the applicable `tests/commands/*.test.ts`.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use rusqlite::Connection;
use serde_json::{Value, json};

struct Env {
    tmp: tempfile::TempDir,
    extra_env: Vec<(String, String)>,
}

struct Out {
    stdout: String,
    stderr: String,
    code: i32,
}

impl Out {
    fn json(&self) -> Value {
        serde_json::from_str(&self.stdout).unwrap_or_else(|e| panic!("bad JSON ({e}): {}", self.stdout))
    }
}

impl Env {
    fn new() -> Env {
        let tmp = tempfile::tempdir().unwrap();
        for d in ["data", "home", "bin"] {
            std::fs::create_dir_all(tmp.path().join(d)).unwrap();
        }
        let env = Env { tmp, extra_env: Vec::new() };
        // No network unless a test opts in.
        env.write_settings(json!({ "ghPrStatus": false, "setupCompleted": true }));
        env
    }

    fn root(&self) -> PathBuf {
        self.tmp.path().canonicalize().unwrap()
    }
    fn data(&self) -> PathBuf {
        self.root().join("data")
    }
    fn home(&self) -> PathBuf {
        self.root().join("home")
    }
    fn bin(&self) -> PathBuf {
        self.root().join("bin")
    }

    fn write_settings(&self, v: Value) {
        std::fs::write(self.tmp.path().join("data/settings.json"), serde_json::to_string_pretty(&v).unwrap() + "\n")
            .unwrap();
    }
    fn settings_text(&self) -> String {
        std::fs::read_to_string(self.data().join("settings.json")).unwrap()
    }
    fn settings(&self) -> Value {
        serde_json::from_str(&self.settings_text()).unwrap()
    }

    fn fake_bin(&self, name: &str, script: &str) {
        let p = self.bin().join(name);
        std::fs::write(&p, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn command(&self, args: &[&str], cwd: Option<&Path>) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_am"));
        c.args(args)
            .env("AM_DATA_DIR", self.data())
            .env("HOME", self.home())
            .env("PATH", format!("{}:/usr/bin:/bin", self.bin().display()))
            .env_remove("AM_HOOK_BIN")
            .env_remove("AM_NO_NATIVE_HOOK")
            .env_remove("AM_LINEAR_API_URL")
            .current_dir(cwd.unwrap_or(&self.root()))
            .stdin(Stdio::null());
        for (k, v) in &self.extra_env {
            c.env(k, v);
        }
        c
    }

    fn run_in(&self, cwd: Option<&Path>, args: &[&str]) -> Out {
        let o = self.command(args, cwd).output().unwrap();
        Out {
            stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
            code: o.status.code().unwrap_or(-1),
        }
    }

    fn run(&self, args: &[&str]) -> Out {
        self.run_in(None, args)
    }

    fn ok(&self, args: &[&str]) -> Out {
        let o = self.run(args);
        assert_eq!(o.code, 0, "am {args:?} failed: {}{}", o.stdout, o.stderr);
        o
    }

    fn db(&self) -> Connection {
        am_core::db::open_at(&self.data().join("agent-monitor.db")).unwrap()
    }

    /// A real git repo with one commit, tracked as `repo` with a main
    /// worktree plus a linked worktree on `feature/login`.
    fn seed_repo(&self) -> (PathBuf, PathBuf) {
        let repo = self.root().join("myrepo");
        std::fs::create_dir_all(&repo).unwrap();
        let git = |args: &[&str], dir: &Path| {
            let ok = Command::new("git")
                .args(args)
                .current_dir(dir)
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@t")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@t")
                .env("HOME", self.home())
                .output()
                .unwrap();
            assert!(ok.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&ok.stderr));
        };
        git(&["init", "-q", "-b", "main"], &repo);
        git(&["commit", "-q", "--allow-empty", "-m", "initial commit"], &repo);
        let wt = self.root().join("myrepo-login");
        git(&["worktree", "add", "-q", "-b", "feature/login", wt.to_str().unwrap()], &repo);
        std::fs::write(wt.join("dirty.txt"), "x").unwrap();

        let conn = self.db();
        conn.execute("INSERT INTO repositories (id, path, name) VALUES ('r1', ?1, 'myrepo')", [repo.to_str().unwrap()])
            .unwrap();
        conn.execute(
            "INSERT INTO worktrees (id, repo_id, path, branch, name, is_main, created_at)
             VALUES ('w-main', 'r1', ?1, 'main', 'main', 1, '2026-01-01 00:00:00')",
            [repo.to_str().unwrap()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO worktrees (id, repo_id, path, branch, name, created_at)
             VALUES ('w-login', 'r1', ?1, 'feature/login', 'login', '2026-02-01 00:00:00')",
            [wt.to_str().unwrap()],
        )
        .unwrap();
        (repo, wt)
    }
}

// --- repo ---

#[test]
fn repo_list_add_remove() {
    let env = Env::new();
    assert!(env.ok(&["repo", "list"]).stdout.contains("No repositories tracked"));

    let not_git = env.root().join("plain");
    std::fs::create_dir_all(&not_git).unwrap();
    let o = env.run(&["repo", "add", not_git.to_str().unwrap()]);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("Not a git repository"));

    let repo = env.root().join("repo1");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    let o = env.ok(&["repo", "add", repo.to_str().unwrap()]);
    assert_eq!(o.stdout.trim(), format!("Added repository: repo1 ({})", repo.display()));
    assert!(env.ok(&["repo", "add", repo.to_str().unwrap()]).stdout.contains("Repository already tracked"));

    let repo2 = env.root().join("repo2");
    std::fs::create_dir_all(repo2.join(".git")).unwrap();
    let added = env.ok(&["repo", "add", repo2.to_str().unwrap(), "--json"]).json();
    assert_eq!(added["name"], "repo2");
    assert!(added["id"].as_str().is_some_and(|s| s.len() == 36));

    let table = env.ok(&["repo", "list"]).stdout;
    assert!(table.starts_with("Name "), "{table}");
    assert!(table.contains("repo1") && table.contains("repo2") && table.contains("Worktrees"));

    let list = env.ok(&["repo", "list", "--json"]).json();
    assert_eq!(list.as_array().unwrap().len(), 2);
    assert_eq!(list[0]["worktreeCount"], 0);

    assert!(env.ok(&["repo", "remove", "repo1"]).stdout.contains("Removed repository: repo1"));
    let o = env.run(&["repo", "remove", "nope"]);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("Repository not found: nope"));
    // By path, relative to the CWD.
    let o = env.run_in(Some(&env.root()), &["repo", "remove", "repo2"]);
    assert!(o.stdout.contains("Removed repository: repo2"));
}

// --- settings ---

#[test]
fn settings_list_and_get() {
    let env = Env::new();
    let list = env.ok(&["settings", "list"]).stdout;
    assert!(list.lines().next().unwrap().starts_with("ide "), "{list}");
    assert!(list.contains("pollingIntervalMs"));
    // Unknown keys from the file are listed too, like `{...DEFAULTS, ...file}`.
    assert!(list.contains("setupCompleted"));

    let json = env.ok(&["settings", "list", "--json"]).json();
    assert_eq!(json["ide"], "cursor");
    assert_eq!(json["ghPrStatus"], false);

    assert_eq!(env.ok(&["settings", "get", "ide"]).stdout, "cursor\n");
    assert_eq!(env.ok(&["settings", "get", "pollingIntervalMs", "--json"]).json(), json!({"key": "pollingIntervalMs", "value": 30000}));

    let o = env.run(&["settings", "get", "nonexistent"]);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("Unknown setting: nonexistent"));
    assert!(o.stderr.contains("Valid keys: ide,"));
}

#[test]
fn settings_set_coerces_and_validates() {
    let env = Env::new();
    assert_eq!(env.ok(&["settings", "set", "ide", "vscode"]).stdout, "ide = vscode\n");
    assert_eq!(env.settings()["ide"], "vscode");

    for (input, expected) in [("true", true), ("0", false), ("1", true), ("false", false)] {
        env.ok(&["settings", "set", "compactView", input]);
        assert_eq!(env.settings()["compactView"], expected, "compactView={input}");
    }
    env.ok(&["settings", "set", "pollingIntervalMs", "5000"]);
    assert_eq!(env.settings()["pollingIntervalMs"], 5000);

    let o = env.run(&["settings", "set", "compactView", "maybe"]);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("boolean"));
    let o = env.run(&["settings", "set", "pollingIntervalMs", "abc"]);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("number"));
    let o = env.run(&["settings", "set", "nonexistent", "x"]);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("Unknown setting"));
    let o = env.run(&["settings", "set", "worktreeSort", "x"]);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("can't be set from the CLI"));
}

#[test]
fn settings_writes_preserve_unknown_keys_order_and_style() {
    let env = Env::new();
    env.write_settings(json!({
        "setupCompleted": true,
        "showIdeStatus": true,
        "ide": "vscode",
        "ghPrStatus": false,
        "lastUpdateCheck": 123
    }));
    env.ok(&["settings", "set", "ide", "terminal"]);
    let text = env.settings_text();
    assert!(text.starts_with("{\n  \"setupCompleted\": true,\n  \"showIdeStatus\": true,\n  \"ide\": \"terminal\","), "{text}");
    assert!(text.ends_with("}\n"));
    let v = env.settings();
    assert_eq!(v["lastUpdateCheck"], 123);
    assert!(v.get("worktreeSort").is_some(), "every known key is written, like the TS saveSettings");

    // Unknown keys can be set (as strings), like the TS command.
    env.ok(&["settings", "set", "showIdeStatus", "yes"]);
    assert_eq!(env.settings()["showIdeStatus"], "yes");

    assert_eq!(env.ok(&["settings", "reset"]).stdout, "Settings reset to defaults.\n");
    let v = env.settings();
    assert_eq!(v["ide"], "cursor");
    assert_eq!(v["ghPrStatus"], true);
    assert_eq!(v["setupCompleted"], true, "reset keeps non-settings keys");
}

// --- hooks ---

#[test]
fn hooks_install_status_uninstall() {
    let mut env = Env::new();
    let claude_settings = env.home().join(".claude/settings.json");
    std::fs::create_dir_all(claude_settings.parent().unwrap()).unwrap();
    std::fs::write(
        &claude_settings,
        serde_json::to_string_pretty(&json!({
            "model": "opus",
            "hooks": {
                "Stop": [{"hooks": [{"type": "command", "command": "echo mine", "timeout": 1}]},
                         {"hooks": [{"type": "command", "command": "cat | am hook-event --worktree \"$CLAUDE_PROJECT_DIR\" --event Stop", "timeout": 5000}]}]
            }
        }))
        .unwrap(),
    )
    .unwrap();
    // A TS-era entry counts as installed.
    assert_eq!(env.ok(&["hooks", "status"]).stdout, "Claude hooks: installed\n");

    let fake_hook = env.bin().join("my-am-hook");
    std::fs::write(&fake_hook, "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&fake_hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    env.extra_env.push(("AM_HOOK_BIN".into(), fake_hook.display().to_string()));

    assert_eq!(env.ok(&["hooks", "install"]).stdout, "Hooks installed into ~/.claude/settings.json\n");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&claude_settings).unwrap()).unwrap();
    assert_eq!(v["model"], "opus");
    let hooks = v["hooks"].as_object().unwrap();
    assert_eq!(hooks.len(), 11);
    let stop = hooks["Stop"].as_array().unwrap();
    assert_eq!(stop.len(), 2, "TS entry replaced, foreign entry kept: {stop:?}");
    assert_eq!(stop[0]["hooks"][0]["command"], "echo mine");
    assert_eq!(
        stop[1]["hooks"][0]["command"],
        format!("\"{}\" hook-event --worktree \"$CLAUDE_PROJECT_DIR\" --event Stop # am hook-event (native)", fake_hook.display())
    );
    let perm = &hooks["PermissionRequest"][0]["hooks"][0];
    assert!(perm["command"].as_str().unwrap().contains("--event PermissionRequest --managed"));
    assert_eq!(perm["timeout"], 300_000);

    // Idempotent.
    env.ok(&["hooks", "install"]);
    let again: Value = serde_json::from_str(&std::fs::read_to_string(&claude_settings).unwrap()).unwrap();
    assert_eq!(again, v);
    assert_eq!(
        env.ok(&["hooks", "status", "--json"]).json(),
        json!({"installed": true, "agents": {"claude": true, "gemini": false}})
    );

    // Opting out of the native binary falls back to the TS command.
    env.extra_env = vec![("AM_NO_NATIVE_HOOK".into(), "1".into())];
    env.ok(&["install-hooks"]);
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&claude_settings).unwrap()).unwrap();
    assert_eq!(v["hooks"]["Stop"][1]["hooks"][0]["command"], "cat | am hook-event --worktree \"$CLAUDE_PROJECT_DIR\" --event Stop");

    assert_eq!(env.ok(&["hooks", "uninstall"]).stdout, "Hooks removed from ~/.claude/settings.json\n");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&claude_settings).unwrap()).unwrap();
    assert_eq!(v, json!({"model": "opus", "hooks": {"Stop": [{"hooks": [{"type": "command", "command": "echo mine", "timeout": 1}]}]}}));
    assert_eq!(env.ok(&["hooks", "status"]).stdout, "Claude hooks: not installed\n");
}

#[test]
fn hooks_install_prefers_am_hook_next_to_the_binary() {
    let env = Env::new();
    env.ok(&["hooks", "install"]);
    let v: Value =
        serde_json::from_str(&std::fs::read_to_string(env.home().join(".claude/settings.json")).unwrap()).unwrap();
    let cmd = v["hooks"]["Stop"][0]["hooks"][0]["command"].as_str().unwrap().to_string();
    let sibling = Path::new(env!("CARGO_BIN_EXE_am")).with_file_name("am-hook");
    if sibling.exists() {
        assert!(cmd.starts_with(&format!("\"{}\" hook-event", sibling.display())), "{cmd}");
    } else {
        assert!(cmd.starts_with("cat | am hook-event"), "{cmd}");
    }
}

#[test]
fn hooks_for_gemini() {
    let mut env = Env::new();
    let fake_hook = env.bin().join("my-am-hook");
    std::fs::write(&fake_hook, "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&fake_hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    env.extra_env.push(("AM_HOOK_BIN".into(), fake_hook.display().to_string()));

    // No ~/.gemini: Gemini is neither installed by default nor listed.
    assert_eq!(env.ok(&["hooks", "install"]).stdout, "Hooks installed into ~/.claude/settings.json\n");
    assert_eq!(env.ok(&["hooks", "status"]).stdout, "Claude hooks: installed\n");
    assert!(!env.home().join(".gemini").exists());

    // Once Gemini CLI has been used, install covers it and keeps its other settings and hooks.
    let gemini_settings = env.home().join(".gemini/settings.json");
    std::fs::create_dir_all(gemini_settings.parent().unwrap()).unwrap();
    let mine = json!({"matcher": "write_file", "hooks": [{"type": "command", "command": "./lint.sh"}]});
    std::fs::write(&gemini_settings, json!({"general": {"vimMode": true}, "hooks": {"BeforeTool": [mine]}}).to_string()).unwrap();
    assert_eq!(env.ok(&["hooks", "status"]).stdout, "Claude hooks: installed\nGemini hooks: not installed\n");
    assert_eq!(
        env.ok(&["hooks", "install"]).stdout,
        "Hooks installed into ~/.claude/settings.json\nHooks installed into ~/.gemini/settings.json\n"
    );
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&gemini_settings).unwrap()).unwrap();
    assert_eq!(v["general"]["vimMode"], true);
    let hooks = v["hooks"].as_object().unwrap();
    assert_eq!(
        hooks.keys().map(String::as_str).collect::<Vec<_>>(),
        ["BeforeTool", "SessionStart", "SessionEnd", "BeforeAgent", "AfterAgent", "AfterTool", "Notification"]
    );
    let before = hooks["BeforeTool"].as_array().unwrap();
    assert_eq!(before[0], mine);
    assert_eq!(before[1]["matcher"], "*");
    assert_eq!(
        before[1]["hooks"][0]["command"],
        format!("\"{}\" hook-event --agent gemini --worktree \"$GEMINI_PROJECT_DIR\" --event BeforeTool", fake_hook.display())
    );
    assert_eq!(env.ok(&["hooks", "status", "--json"]).json()["agents"], json!({"claude": true, "gemini": true}));

    // --agent narrows uninstall; the user's own Gemini hook stays.
    assert_eq!(env.ok(&["hooks", "uninstall", "--agent", "gemini"]).stdout, "Hooks removed from ~/.gemini/settings.json\n");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&gemini_settings).unwrap()).unwrap();
    assert_eq!(v, json!({"general": {"vimMode": true}, "hooks": {"BeforeTool": [mine]}}));
    assert_eq!(env.ok(&["hooks", "status"]).stdout, "Claude hooks: installed\nGemini hooks: not installed\n");

    // Gemini allows comments in settings.json: refuse rather than rewrite (and lose) them.
    let commented = "{\n  // my settings\n  \"general\": {}\n}\n";
    std::fs::write(&gemini_settings, commented).unwrap();
    let o = env.run(&["hooks", "install", "--agent", "gemini"]);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("Failed to install Gemini CLI hooks: can't parse"), "{}", o.stderr);
    assert_eq!(std::fs::read_to_string(&gemini_settings).unwrap(), commented);

    let o = env.run(&["hooks", "install", "--agent", "codex"]);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("Unknown agent: codex. Valid agents: claude, gemini"), "{}", o.stderr);
}

/// A JSON manifest in `<data>/agents/` is a full agent plugin: no code.
#[test]
fn manifest_plugins_need_no_code() {
    let mut env = Env::new();
    let fake_hook = env.bin().join("my-am-hook");
    std::fs::write(&fake_hook, "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&fake_hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    env.extra_env.push(("AM_HOOK_BIN".into(), fake_hook.display().to_string()));
    std::fs::create_dir_all(env.data().join("agents")).unwrap();
    std::fs::write(
        env.data().join("agents/mycli.json"),
        json!({
            "id": "mycli", "name": "My CLI", "shortName": "MyCLI", "program": "mycli", "configDir": "~/.mycli",
            "hooks": {"settingsPath": "~/.mycli/hooks.json", "projectDirVar": "MYCLI_DIR"},
            "events": {"Prompt": "UserPromptSubmit", "Done": "Stop"}
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(env.data().join("agents/broken.json"), "{").unwrap();

    // Installed by default once the CLI has been used here.
    std::fs::create_dir_all(env.home().join(".mycli")).unwrap();
    assert_eq!(
        env.ok(&["hooks", "install"]).stdout,
        "Hooks installed into ~/.claude/settings.json\nHooks installed into ~/.mycli/hooks.json\n"
    );
    let v: Value = serde_json::from_str(&std::fs::read_to_string(env.home().join(".mycli/hooks.json")).unwrap()).unwrap();
    assert_eq!(
        v["hooks"]["Done"][0]["hooks"][0]["command"],
        format!("\"{}\" hook-event --agent mycli --worktree \"$MYCLI_DIR\" --event Done", fake_hook.display())
    );
    assert_eq!(env.ok(&["hooks", "status"]).stdout, "Claude hooks: installed\nMyCLI hooks: installed\n");
    env.ok(&["settings", "set", "defaultAgent", "mycli"]);
    let o = env.run(&["settings", "set", "defaultAgent", "nope"]);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("Available: claude, gemini, mycli"), "{}", o.stderr);

    // Doctor reports the plugin's hooks and the file it skipped.
    let doctor = env.ok(&["doctor"]).stdout;
    assert!(doctor.contains("MyCLI hooks: Installed in ~/.mycli/hooks.json"), "{doctor}");
    assert!(doctor.contains("Agent plugins: Skipped in") && doctor.contains("broken.json"), "{doctor}");

    // Disabled plugins are left out of a default install and refuse an explicit one.
    env.ok(&["settings", "set", "defaultAgent", "claude"]);
    let settings_path = env.data().join("settings.json");
    let mut st: Value = serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
    st["disabledAgents"] = json!(["mycli"]);
    std::fs::write(&settings_path, st.to_string()).unwrap();
    assert_eq!(env.ok(&["hooks", "install"]).stdout, "Hooks installed into ~/.claude/settings.json\n");
    let o = env.run(&["hooks", "install", "--agent", "mycli"]);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("My CLI is disabled"), "{}", o.stderr);
}

// --- status ---

#[test]
fn status_get_and_set() {
    let env = Env::new();
    let o = env.run(&["status", "--worktree", "/tmp/nonexistent"]);
    assert_eq!(o.code, 1);
    assert!(o.stdout.contains("No worktree found in DB for: /tmp/nonexistent"));

    let o = env.run(&["status"]);
    assert_eq!(o.code, 1, "missing --worktree is a usage error");

    let (repo, _) = env.seed_repo();
    let repo_s = repo.to_str().unwrap();
    let out = env.ok(&["status", "-w", repo_s]).stdout;
    assert!(out.contains("Status:   unknown") && out.contains("Branch:   main"), "{out}");

    assert_eq!(env.ok(&["status", "-w", repo_s, "--set", "executing"]).stdout, format!("Status set to \"executing\" for {repo_s}\n"));
    let out = env.ok(&["status", "-w", repo_s]).stdout;
    assert!(out.contains("Status:   executing") && out.contains("Open:     yes"), "{out}");

    let o = env.run(&["status", "-w", repo_s, "--set", "invalid"]);
    assert_eq!(o.code, 1);
    assert!(o.stdout.contains("Invalid status: invalid. Must be one of: idle, executing, planning, waiting, delegating, done"));
}

// --- hook-event ---

#[test]
fn hook_event_runs_the_shared_handler() {
    let env = Env::new();
    let (_, wt) = env.seed_repo();
    let send = |event: &str, extra: &[&str]| {
        let mut args = vec!["hook-event", "--worktree", wt.to_str().unwrap(), "--event", event];
        args.extend_from_slice(extra);
        let mut child = env.command(&args, None).stdin(Stdio::piped()).spawn().unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(json!({"hook_event_name": event, "session_id": "s1"}).to_string().as_bytes())
            .unwrap();
        assert!(child.wait().unwrap().success());
    };
    send("UserPromptSubmit", &[]);
    let status: String =
        env.db().query_row("SELECT status FROM agent_status WHERE worktree_id = 'w-login'", [], |r| r.get(0)).unwrap();
    assert_eq!(status, "executing");
    send("PermissionRequest", &["--managed"]);
    let (status, session): (String, String) = env
        .db()
        .query_row("SELECT status, session_id FROM agent_status WHERE worktree_id = 'w-login'", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!((status.as_str(), session.as_str()), ("waiting", "s1"));
}

// --- worktree list / info / resolve ---

#[test]
fn worktree_list_table_and_json() {
    let env = Env::new();
    let o = env.run(&["ls"]);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("No repositories tracked"));

    let (repo, wt) = env.seed_repo();
    env.db()
        .execute("INSERT INTO agent_status (worktree_id, status, is_open) VALUES ('w-login', 'waiting', 1)", [])
        .unwrap();
    let table = env.ok(&["worktree", "list"]).stdout;
    let lines: Vec<&str> = table.lines().collect();
    assert!(lines[0].starts_with("Branch") && lines[0].contains("Agent") && lines[0].contains("Last Commit"), "{table}");
    assert!(!lines[0].contains("PR"), "no PR column without PR data: {table}");
    // Default sort: dedicated worktrees before the main checkout.
    assert!(lines[2].starts_with("feature/login") && lines[2].contains("waiting") && lines[2].contains("1M"), "{table}");
    assert!(lines[3].starts_with("main") && lines[3].contains("clean") && lines[3].contains("ago"), "{table}");

    let json = env.ok(&["ls", "--json", "--repo", repo.to_str().unwrap()]).json();
    let rows = json.as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["branch"], "feature/login");
    assert_eq!(rows[0]["path"], wt.to_str().unwrap());
    assert_eq!(rows[0]["repo"], "myrepo");
    assert_eq!(rows[0]["agent"], "waiting");
    assert_eq!(rows[0]["git"]["dirty"], 1);
    assert_eq!(rows[0]["lastCommit"]["message"], "initial commit");
    assert_eq!(rows[0]["pr"], Value::Null);
    assert_eq!(rows[1]["agent"], "unknown");
}

#[test]
fn worktree_info_resolves_targets() {
    let env = Env::new();
    let (repo, wt) = env.seed_repo();
    let repo_s = repo.to_str().unwrap();

    // Partial branch match, repo detected from the CWD (inside a linked worktree).
    let o = env.run_in(Some(&wt), &["worktree", "info", "login"]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(o.stdout.starts_with("Branch:  "), "{}", o.stdout);
    assert!(o.stdout.contains("feature/login") && o.stdout.contains("Git:     1 modified"), "{}", o.stdout);
    assert!(o.stdout.contains("Commit:") && o.stdout.contains("initial commit"));

    let j = env.ok(&["wt", "info", wt.to_str().unwrap(), "--repo", repo_s, "--json"]).json();
    assert_eq!(j["branch"], "feature/login");
    assert_eq!(j["exists"], true);
    assert_eq!(j["agent"], Value::Null);
    assert_eq!(j["git"]["dirty"], 1);

    let o = env.run(&["worktree", "info", "nope", "--repo", repo_s]);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("Worktree not found: nope"));

    let o = env.run(&["worktree", "info", "main"]);
    assert_eq!(o.code, 1, "no --repo and CWD outside any repo");
    assert!(o.stderr.contains("Could not detect repository from CWD"));

    let o = env.run(&["worktree", "info", "main", "--repo", "/not/tracked"]);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("Repository not tracked: /not/tracked"));
}

#[test]
fn worktree_open_uses_the_configured_ide() {
    let env = Env::new();
    let (repo, wt) = env.seed_repo();
    let log = env.root().join("opened.txt");
    env.fake_bin("code", &format!("echo \"$@\" >> {}", log.display()));
    env.ok(&["settings", "set", "ide", "vscode"]);
    let o = env.ok(&["open", "feature/login", "--repo", repo.to_str().unwrap()]);
    assert_eq!(o.stdout, "Opened feature/login in vscode\n");
    assert_eq!(std::fs::read_to_string(&log).unwrap().trim(), wt.to_str().unwrap());
}

// --- pr / linear ---

fn gql_pr(branch: &str) -> String {
    json!({"data": {"repository": {"b0": {"nodes": [{
        "number": 42, "title": "Add login", "url": "https://github.com/o/r/pull/42",
        "state": "OPEN", "isDraft": true, "reviewDecision": null, "headRefName": branch,
        "isCrossRepository": false,
        "commits": {"nodes": [{"commit": {"statusCheckRollup": {"contexts": {"nodes": [
            {"__typename": "CheckRun", "name": "ci", "status": "COMPLETED", "conclusion": "SUCCESS"}
        ]}}}}]}
    }]}}}})
    .to_string()
}

#[test]
fn pr_show_with_fake_gh() {
    let env = Env::new();
    let (repo, wt) = env.seed_repo();
    let response = env.root().join("gh.json");
    std::fs::write(&response, gql_pr("feature/login")).unwrap();
    env.fake_bin("gh", &format!("cat {}", response.display()));
    let repo_s = repo.to_str().unwrap();

    let out = env.ok(&["pr", "login", "--repo", repo_s]).stdout;
    assert!(out.starts_with("PR:      #42 Add login\n"), "{out}");
    assert!(out.contains("Status:  Draft") && out.contains("Draft:   yes") && out.contains("URL:     https://github.com/o/r/pull/42"));

    let j = env.ok(&["pr", "show", "login", "--repo", repo_s, "--json"]).json();
    assert_eq!(j["number"], 42);
    assert_eq!(j["isDraft"], true);

    // No target: the CWD's branch.
    let o = env.run_in(Some(&wt), &["pr", "--json"]);
    assert_eq!(o.json()["number"], 42, "{}", o.stderr);

    std::fs::write(&response, r#"{"data":{"repository":{"b0":{"nodes":[]}}}}"#).unwrap();
    assert_eq!(env.ok(&["pr", "login", "--repo", repo_s]).stdout, "No PR found for branch: feature/login\n");
    assert_eq!(env.ok(&["pr", "open", "login", "--repo", repo_s]).stdout, "No PR found for branch: feature/login\n");
}

/// One-request-per-connection HTTP server answering every POST with `body`.
fn serve_json(body: String) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut len = 0usize;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap_or(0);
                }
            }
            let mut buf = vec![0; len];
            let _ = reader.read_exact(&mut buf);
            let mut s = stream;
            let _ = write!(
                s,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    format!("http://{addr}/graphql")
}

#[test]
fn linear_requires_configuration_then_shows_ticket() {
    let mut env = Env::new();
    let (repo, _) = env.seed_repo();
    let repo_s = repo.to_str().unwrap();
    let o = env.run(&["linear", "login", "--repo", repo_s]);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("Linear integration not configured"));

    env.write_settings(json!({"ghPrStatus": false, "linearEnabled": true, "linearApiKey": "lin_test"}));
    let issue = json!({"data": {"b0": {
        "identifier": "ENG-7", "title": "Login page", "url": "https://linear.app/t/issue/ENG-7",
        "state": {"name": "In Progress", "color": "#0ea5e9", "type": "started"},
        "priorityLabel": "High", "assignee": {"name": "Tom"}, "project": null,
        "attachments": {"nodes": []}
    }}});
    env.extra_env.push(("AM_LINEAR_API_URL".into(), serve_json(issue.to_string())));

    let out = env.ok(&["linear", "login", "--repo", repo_s]).stdout;
    assert!(out.starts_with("Ticket:    ENG-7 Login page\n"), "{out}");
    assert!(out.contains("Status:    In Progress") && out.contains("Priority:  High") && out.contains("Assignee:  Tom"), "{out}");
    let j = env.ok(&["linear", "show", "login", "--repo", repo_s, "--json"]).json();
    assert_eq!(j["identifier"], "ENG-7");
    assert_eq!(j["state"]["name"], "In Progress");
}

// --- script ---

#[test]
fn script_edit_show_remove() {
    let env = Env::new();
    let (repo, _) = env.seed_repo();
    let repo_s = repo.to_str().unwrap();
    assert_eq!(env.ok(&["script", "show", "--repo", repo_s]).stdout, "No startup script for myrepo\n");
    assert_eq!(env.ok(&["script", "remove", "--repo", repo_s]).stdout, "No startup script for myrepo\n");

    let log = env.root().join("edited.txt");
    env.fake_bin("cursor", &format!("echo \"$@\" >> {}", log.display()));
    assert_eq!(env.ok(&["script", "edit", "--repo", repo_s]).stdout, "Opened startup script for myrepo in cursor\n");
    let path = env.data().join("scripts/r1.sh");
    assert_eq!(std::fs::read_to_string(&log).unwrap().trim(), path.to_str().unwrap());
    assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o755);

    let out = env.ok(&["script", "show", "--repo", repo_s]).stdout;
    assert!(out.starts_with("# Startup script for myrepo\n# Path: "), "{out}");
    assert!(out.contains("echo \"Startup script finished.\""));
    let j = env.ok(&["script", "show", "--repo", repo_s, "--json"]).json();
    assert_eq!(j["repo"], "myrepo");
    assert!(j["content"].as_str().unwrap().starts_with("#!/bin/bash"));

    assert_eq!(env.ok(&["script", "remove", "--repo", repo_s]).stdout, "Removed startup script for myrepo\n");
    assert!(!path.exists());
}

// --- doctor / logs / stubs ---

#[test]
fn doctor_reports_checks() {
    let env = Env::new();
    let out = env.ok(&["doctor"]).stdout;
    assert!(out.contains("[OK]    App directory:"), "{out}");
    assert!(out.contains("[OK]    Database:") && out.contains("0 repos, 0 worktrees"));
    assert!(out.contains("[WARN]  Claude hooks: Not installed. Run: am hooks install"));
    let j = env.ok(&["doctor", "--json"]).json();
    let names: Vec<&str> = j.as_array().unwrap().iter().map(|c| c["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"Database") && names.contains(&"gh CLI") && names.contains(&"Claude hooks"));
    assert!(!names.contains(&"Linear"));
}

#[test]
fn logs_filter_tail_and_clear() {
    let env = Env::new();
    assert!(env.ok(&["logs"]).stdout.starts_with("No log file found at "));
    let log = env.data().join("debug.log");
    std::fs::write(
        &log,
        "[t1] [DEBUG] [git] one\n[t2] [INFO] [github] two\n[t3] [WARN] [github] three\n[t4] [ERROR] [linear] four\n",
    )
    .unwrap();
    assert_eq!(env.ok(&["logs", "-n", "2"]).stdout, "[t3] [WARN] [github] three\n[t4] [ERROR] [linear] four\n");
    assert_eq!(env.ok(&["logs", "--level", "warn"]).stdout, "[t3] [WARN] [github] three\n[t4] [ERROR] [linear] four\n");
    assert_eq!(env.ok(&["logs", "--module", "github", "--lines", "1"]).stdout, "[t3] [WARN] [github] three\n");
    assert_eq!(env.ok(&["logs", "--clear"]).stdout, "Log file cleared.\n");
    assert_eq!(std::fs::metadata(&log).unwrap().len(), 0);
}

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(["-c", "user.email=t@t", "-c", "user.name=t", "-c", "init.defaultBranch=main"])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap()
        .status
        .success();
    assert!(ok, "git {args:?}");
}

#[test]
fn worktree_create_delete_sync_end_to_end() {
    let env = Env::new();
    let root = env.root();
    git(&root, &["init", "-q", "--bare", "origin.git"]);
    git(&root, &["init", "-q", "repo"]);
    let repo = root.join("repo");
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "init"]);
    git(&repo, &["remote", "add", "origin", "../origin.git"]);
    git(&repo, &["push", "-q", "-u", "origin", "main"]);
    git(&repo, &["push", "-q", "origin", "main:refs/heads/feature/remote"]);
    let r = repo.to_str().unwrap();
    env.ok(&["repo", "add", r]);

    let o = env.ok(&["new", "feature/x", "--repo", r, "--name", "Nick"]);
    assert!(o.stdout.contains("Created worktree: ") && o.stdout.contains("Branch: feature/x (based on main)"), "{}", o.stdout);
    let j = env.ok(&["ls", "--repo", r, "--json"]).json();
    assert!(j.to_string().contains("feature/x"), "{j}");

    let o = env.run(&["worktree", "create", "feature/remote", "--repo", r]);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("already exists on origin") && o.stderr.contains("--track"), "{}", o.stderr);
    let o = env.ok(&["worktree", "create", "feature/remote", "--repo", r, "--track", "--json"]);
    assert_eq!(o.json()["mode"], "track");

    let o = env.ok(&["worktree", "delete", "feature/x", "--repo", r, "--delete-branch", "--force"]);
    assert!(o.stdout.contains("Deleted worktree: ") && o.stdout.contains("Deleted local branch: feature/x"), "{}", o.stdout);
    assert!(!env.ok(&["ls", "--repo", r, "--json"]).json().to_string().contains("feature/x"));

    // A worktree made behind am's back appears after `wt sync`.
    git(&repo, &["worktree", "add", "-q", "-b", "ext", "../ext-wt"]);
    assert!(env.ok(&["wt", "sync", "--repo", r]).stdout.starts_with("Synced: repo"));
    assert!(env.ok(&["ls", "--repo", r, "--json"]).json().to_string().contains("\"ext\""));

    assert_eq!(env.run(&["--version"]).code, 0);
}

#[test]
fn cut_commands_are_gone() {
    let env = Env::new();
    // Cut commands are gone, and usage errors exit 1 like commander.
    for args in [&["daemon", "start"][..], &["agent", "send", "x", "hi"], &["bogus"]] {
        assert_eq!(env.run(args).code, 1, "{args:?}");
    }
}

#[test]
fn theme_list_show_export_import_use_remove() {
    let env = Env::new();
    let list = env.ok(&["theme", "list", "--json"]).json();
    let names: Vec<&str> = list.as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    for n in ["ansi", "dracula", "catppuccin-latte", "dos", "ibm-3270", "amber-phosphor"] {
        assert!(names.contains(&n), "{n} missing from {names:?}");
    }
    assert_eq!(list[0]["active"], true, "ansi is the default");

    let shown = env.ok(&["theme", "show", "dos", "--json"]).json();
    assert_eq!(shown["colors"]["background"], "#0000aa");
    assert!(env.ok(&["theme", "show", "nord"]).stdout.contains("accent"));
    assert_eq!(env.run(&["theme", "show", "nope"]).code, 1);

    // Export a built-in as a template, tweak it, import it as a custom theme.
    let file = env.root().join("mine.json");
    env.ok(&["theme", "export", "gruvbox", "-o", file.to_str().unwrap()]);
    let mut t: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(t["colors"]["selection"], "#3c3836");
    t["name"] = "mine".into();
    t["colors"]["accent"] = "#ff00ff".into();
    std::fs::write(&file, t.to_string()).unwrap();
    env.ok(&["theme", "import", file.to_str().unwrap()]);
    assert!(env.data().join("themes/mine.json").is_file());
    assert_eq!(env.ok(&["theme", "show", "mine", "--json"]).json()["colors"]["accent"], "#ff00ff");
    let o = env.run(&["theme", "import", file.to_str().unwrap()]);
    assert!(o.code == 1 && o.stderr.contains("already exists"), "{}", o.stderr);
    env.ok(&["theme", "import", file.to_str().unwrap(), "--force"]);

    // Import is strict about typos.
    let bad = env.root().join("bad.json");
    std::fs::write(&bad, r##"{"name":"bad","colors":{"acent":"#fff","red":"crimson","green":"reverse"}}"##).unwrap();
    let o = env.run(&["theme", "import", bad.to_str().unwrap()]);
    assert_eq!(o.code, 1);
    for needle in ["acent", "crimson", "only works for selection"] {
        assert!(o.stderr.contains(needle), "{needle} not reported: {}", o.stderr);
    }
    let o = env.run(&["theme", "import", file.to_str().unwrap(), "--name", "nord"]);
    assert!(o.code == 1 && o.stderr.contains("built-in"), "{}", o.stderr);

    env.ok(&["theme", "use", "mine"]);
    assert_eq!(env.settings()["theme"], "mine");
    assert_eq!(env.settings()["setupCompleted"], true, "other keys kept");
    let o = env.run(&["settings", "set", "theme", "bogus"]);
    assert!(o.code == 1 && o.stderr.contains("Unknown theme"), "{}", o.stderr);
    env.ok(&["settings", "set", "theme", "dracula"]);
    env.ok(&["theme", "use", "mine"]);

    assert_eq!(env.run(&["theme", "remove", "nord"]).code, 1);
    env.ok(&["theme", "remove", "mine"]);
    assert!(!env.data().join("themes/mine.json").exists());
    assert_eq!(env.settings()["theme"], "ansi", "removing the active theme falls back to ansi");
}
