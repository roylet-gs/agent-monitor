//! Integration tests for `am`: run the real binary against an isolated
//! `AM_DATA_DIR` and a temp `HOME` (never the real ~/.agent-monitor or
//! ~/.claude), with fake `gh`/editor binaries first on `PATH`. Ported from
//! the applicable `tests/commands/*.test.ts`.

// Seeding DBs in this process must not write the real debug.log (the `am` children log to their temp dir).
am_core::silence_unit_tests!();

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

/// Environment variables that move an agent CLI's config or data dir.
/// Cleared for every `am` run so a developer's own setting can't leak in.
const AGENT_ENV: &[&str] = &[
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "GROK_HOME",
    "HERMES_HOME",
    "KIMI_CODE_HOME",
    "KIMI_SHARE_DIR",
    "KIRO_HOME",
    "OPENCODE_CONFIG_DIR",
    "OPENCODE_DB",
    "PI_CODING_AGENT_DIR",
    "PI_CONFIG_DIR",
    "PI_CODING_AGENT_SESSION_DIR",
    "OMP_AGENT_DIR",
    "CAMPFIRE_CODING_AGENT_DIR",
    "CODEBUDDY_CONFIG_DIR",
];

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
        // Whatever the machine running the tests has: agent config dirs come
        // from $HOME unless a test sets one through `extra_env`.
        for var in AGENT_ENV {
            c.env_remove(var);
        }
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
    assert_eq!(json["ide"], "vscode");
    assert_eq!(json["ideCommand"], "");
    assert_eq!(json["ghPrStatus"], false);

    assert_eq!(env.ok(&["settings", "get", "ide"]).stdout, "vscode\n");
    assert_eq!(env.ok(&["settings", "get", "pollingIntervalMs", "--json"]).json(), json!({"key": "pollingIntervalMs", "value": 30000}));

    let o = env.run(&["settings", "get", "nonexistent"]);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("Unknown setting: nonexistent"));
    assert!(o.stderr.contains("Valid keys: ide,"));
}

#[test]
fn settings_set_coerces_and_validates() {
    let env = Env::new();
    assert_eq!(env.ok(&["settings", "set", "ide", "zed"]).stdout, "ide = zed\n");
    assert_eq!(env.settings()["ide"], "zed");
    let o = env.run(&["settings", "set", "ide", "notepad"]);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("Use one of: vscode, cursor, zed, windsurf, terminal, custom"), "{}", o.stderr);
    env.ok(&["settings", "set", "ideCommand", "open -a Nova {path}"]);
    assert_eq!(env.settings()["ideCommand"], "open -a Nova {path}");

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
    let o = env.run(&["settings", "set", "keyBindings", "x"]);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("Key bindings"));

    assert_eq!(env.settings()["contextWindowTokens"], 1_000_000, "1M by default");
    for (input, expected) in [("200k", 200_000), ("1.5m", 1_500_000), ("128000", 128_000)] {
        env.ok(&["settings", "set", "contextWindowTokens", input]);
        assert_eq!(env.settings()["contextWindowTokens"], expected, "contextWindowTokens={input}");
    }
    for bad in ["abc", "500", "0"] {
        let o = env.run(&["settings", "set", "contextWindowTokens", bad]);
        assert_eq!(o.code, 1, "{bad}");
        assert!(o.stderr.contains("Invalid token count"));
    }

    env.ok(&["settings", "set", "dailyTokensScope", "visible"]);
    assert_eq!(env.settings()["dailyTokensScope"], "visible");
    let o = env.run(&["settings", "set", "dailyTokensScope", "some"]);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("Use all or visible"));

    env.ok(&["settings", "set", "dailyTokensMode", "cost"]);
    assert_eq!(env.settings()["dailyTokensMode"], "cost");
    let o = env.run(&["settings", "set", "dailyTokensMode", "dollars"]);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("Use one of: tokens, weighted, cost, breakdown"));

    env.ok(&["settings", "set", "reviewRequestsScope", "teams"]);
    assert_eq!(env.settings()["reviewRequestsScope"], "teams");
    let o = env.run(&["settings", "set", "reviewRequestsScope", "everyone"]);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("Use one of: all, direct, teams"));
    env.ok(&["settings", "set", "reviewSort", "oldest"]);
    let o = env.run(&["settings", "set", "reviewSort", "random"]);
    assert!(o.code == 1 && o.stderr.contains("Use one of: direct, repo, updated, oldest"), "{}", o.stderr);
    env.ok(&["settings", "set", "reviewExcludeRepos", "docs, acme/web"]);
    assert_eq!(env.settings()["reviewExcludeRepos"], "docs, acme/web");

    env.ok(&["settings", "set", "appMode", "reviews"]);
    assert_eq!(env.settings()["appMode"], "reviews");
    let o = env.run(&["settings", "set", "appMode", "dashboard"]);
    assert!(o.code == 1 && o.stderr.contains("Use one of: full, reviews"), "{}", o.stderr);
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
    assert_eq!(v["ide"], "vscode");
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
        json!({"installed": true, "agents": hooks_installed(&["claude"])})
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
fn hooks_install_prefers_an_installed_am_hook_over_a_dev_build() {
    // The test binary lives in cargo's target/ dir, i.e. a dev build that may
    // sit in a worktree about to be deleted.
    let env = Env::new();
    env.fake_bin("am-hook", "exit 0");
    env.ok(&["hooks", "install"]);
    let v: Value =
        serde_json::from_str(&std::fs::read_to_string(env.home().join(".claude/settings.json")).unwrap()).unwrap();
    let cmd = v["hooks"]["Stop"][0]["hooks"][0]["command"].as_str().unwrap();
    assert!(cmd.starts_with(&format!("\"{}\" hook-event", env.bin().join("am-hook").display())), "{cmd}");
}

#[test]
fn doctor_flags_hooks_pointing_at_a_missing_binary() {
    let env = Env::new();
    let claude_settings = env.home().join(".claude/settings.json");
    std::fs::create_dir_all(claude_settings.parent().unwrap()).unwrap();
    let cmd = "\"/gone/target/debug/am-hook\" hook-event --worktree \"$CLAUDE_PROJECT_DIR\" --event Stop # am hook-event (native)";
    std::fs::write(&claude_settings, json!({"hooks": {"Stop": [{"hooks": [{"type": "command", "command": cmd}]}]}}).to_string())
        .unwrap();
    let out = env.ok(&["doctor"]).stdout;
    assert!(out.contains("[WARN]  Claude hooks: Hooks in ~/.claude/settings.json run a missing /gone/target/debug/am-hook"), "{out}");
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
    assert_eq!(env.ok(&["hooks", "status", "--json"]).json()["agents"], hooks_installed(&["claude", "gemini"]));

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

    let o = env.run(&["hooks", "install", "--agent", "aider"]);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains(&format!("Unknown agent: aider. Valid agents: {}", builtin_ids().join(", "))), "{}", o.stderr);
}

/// Rovo Dev's hooks are a marked block of list items in
/// `~/.rovodev/config.yml`'s `eventHooks.events` (the keys are added outside
/// it when missing, never a second `eventHooks:`). Reinstalling is a no-op
/// and uninstalling restores the file; a user's hook for one of am's events,
/// and am's entries without their marker lines, are refused.
#[test]
fn hooks_for_rovodev() {
    let mut env = Env::new();
    let fake_hook = env.bin().join("my-am-hook");
    std::fs::write(&fake_hook, "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&fake_hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    env.extra_env.push(("AM_HOOK_BIN".into(), fake_hook.display().to_string()));
    let config = env.home().join(".rovodev/config.yml");
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    let cmd = |event: &str| format!("\"\\\"{}\\\" hook-event --agent rovodev --worktree \\\"$PWD\\\" --event {event}\"", fake_hook.display());

    // No eventHooks: the keys, then the block, go at the end.
    let user = "agent:\n  streaming: true\nsessions:\n  auto_restore: false\n";
    std::fs::write(&config, user).unwrap();
    assert_eq!(env.ok(&["hooks", "install", "--agent", "rovodev"]).stdout, "Hooks installed into ~/.rovodev/config.yml\n");
    let text = std::fs::read_to_string(&config).unwrap();
    let head = format!(
        "{user}\neventHooks:\n  events:\n    # agent-monitor hooks rovodev begin\n    - name: on_session_start\n      commands:\n        - command: {}\n",
        cmd("on_session_start")
    );
    assert!(text.starts_with(&head), "{text}");
    assert!(text.ends_with(&format!("    - name: on_session_end\n      commands:\n        - command: {}\n    # agent-monitor hooks rovodev end\n", cmd("on_session_end"))), "{text}");
    assert_eq!(text.matches("- name: ").count(), 8);
    assert_eq!(env.ok(&["hooks", "status", "--json"]).json()["agents"]["rovodev"], true);
    env.ok(&["hooks", "install", "--agent", "rovodev"]);
    assert_eq!(std::fs::read_to_string(&config).unwrap(), text, "reinstalling changes nothing");
    assert_eq!(env.ok(&["hooks", "uninstall", "--agent", "rovodev"]).stdout, "Hooks removed from ~/.rovodev/config.yml\n");
    assert_eq!(std::fs::read_to_string(&config).unwrap(), user, "the empty keys go too");

    // An item the user adds after am's keeps its keys on uninstall.
    env.ok(&["hooks", "install", "--agent", "rovodev"]);
    let theirs = "    - name: on_custom\n      commands:\n        - command: say hi\n";
    std::fs::write(&config, format!("{}{theirs}", std::fs::read_to_string(&config).unwrap())).unwrap();
    env.ok(&["hooks", "uninstall", "--agent", "rovodev"]);
    assert_eq!(std::fs::read_to_string(&config).unwrap(), format!("{user}\neventHooks:\n  events:\n{theirs}"));

    // The user's own eventHooks: am's events go inside their list.
    let user = "eventHooks:\n  logFile: ~/.rovodev/event_hooks.log\n  events:\n    - name: on_custom\n      commands:\n        - command: afplay /System/Library/Sounds/Glass.aiff\nsessions:\n  auto_restore: false\n";
    std::fs::write(&config, user).unwrap();
    env.ok(&["hooks", "install", "--agent", "rovodev"]);
    let text = std::fs::read_to_string(&config).unwrap();
    assert!(
        text.starts_with(&format!(
            "eventHooks:\n  logFile: ~/.rovodev/event_hooks.log\n  events:\n    # agent-monitor hooks rovodev begin\n    - name: on_session_start\n      commands:\n        - command: {}\n",
            cmd("on_session_start")
        )),
        "{text}"
    );
    assert!(text.ends_with("    # agent-monitor hooks rovodev end\n    - name: on_custom\n      commands:\n        - command: afplay /System/Library/Sounds/Glass.aiff\nsessions:\n  auto_restore: false\n"), "{text}");
    assert_eq!(text.matches("eventHooks:").count(), 1);
    env.ok(&["hooks", "install", "--agent", "rovodev"]);
    assert_eq!(std::fs::read_to_string(&config).unwrap(), text, "reinstalling changes nothing");
    assert!(env.ok(&["doctor"]).stdout.contains("Rovo Dev hooks: Installed in ~/.rovodev/config.yml"));
    // Rovo Dev rewrote the file and dropped the marker comments: still
    // installed, never installed twice, and uninstall says what to remove.
    let stripped: String = text.lines().filter(|l| !l.contains("agent-monitor hooks")).map(|l| format!("{l}\n")).collect();
    std::fs::write(&config, &stripped).unwrap();
    assert_eq!(env.ok(&["hooks", "status", "--json"]).json()["agents"]["rovodev"], true);
    let o = env.run(&["hooks", "install", "--agent", "rovodev"]);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("outside am's `agent-monitor hooks rovodev` marker lines"), "{}", o.stderr);
    assert_eq!(env.run(&["hooks", "uninstall", "--agent", "rovodev"]).code, 1);
    assert_eq!(std::fs::read_to_string(&config).unwrap(), stripped);
    std::fs::write(&config, &text).unwrap();
    env.ok(&["hooks", "uninstall", "--agent", "rovodev"]);
    assert_eq!(std::fs::read_to_string(&config).unwrap(), user);

    // A hook of the user's for one of am's events: Rovo Dev would run only one.
    let user = "eventHooks:\n  events:\n    - name: on_complete\n      commands:\n        - command: afplay /System/Library/Sounds/Glass.aiff\n";
    std::fs::write(&config, user).unwrap();
    let o = env.run(&["hooks", "install", "--agent", "rovodev"]);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("already has hooks for on_complete;"), "{}", o.stderr);
    assert_eq!(std::fs::read_to_string(&config).unwrap(), user);

    // An inline list am can't extend without editing the user's line: refused, file untouched.
    let user = "eventHooks:\n  events: []\n";
    std::fs::write(&config, user).unwrap();
    let o = env.run(&["hooks", "install", "--agent", "rovodev"]);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("inline `eventHooks.events` value"), "{}", o.stderr);
    assert_eq!(std::fs::read_to_string(&config).unwrap(), user);
}

/// Kimi Code keeps its hooks in TOML: am owns a marked block of `[[hooks]]`
/// tables and never touches the rest of the file.
#[test]
fn hooks_for_kimi() {
    let mut env = Env::new();
    let fake_hook = env.bin().join("my-am-hook");
    std::fs::write(&fake_hook, "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&fake_hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    env.extra_env.push(("AM_HOOK_BIN".into(), fake_hook.display().to_string()));

    // The user's config ends inside a table; `[[hooks]]` after it is still top level.
    let config = env.home().join(".kimi-code/config.toml");
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    let user = "default_model = \"kimi-k2\"\n\n[[hooks]]\nevent = \"Notification\"\nmatcher = \"task\\\\.completed\"\ncommand = \"notify\"\n\n[ui]\ntheme = \"dark\"\n";
    std::fs::write(&config, user).unwrap();
    assert!(env.ok(&["hooks", "status"]).stdout.contains("Kimi hooks: not installed\n"));

    assert_eq!(env.ok(&["hooks", "install", "--agent", "kimi"]).stdout, "Hooks installed into ~/.kimi-code/config.toml\n");
    let text = std::fs::read_to_string(&config).unwrap();
    let block = text.strip_prefix(user).expect("the user's text stays as it was, block appended");
    let cmd = |event: &str| format!("\"\\\"{}\\\" hook-event --agent kimi --worktree \\\"\\\" --event {event}\"", fake_hook.display());
    assert!(block.starts_with(&format!("\n# agent-monitor hooks kimi begin\n[[hooks]]\nevent = \"SessionStart\"\ncommand = {}\ntimeout = 5\n\n[[hooks]]\n", cmd("SessionStart"))), "{block}");
    assert!(block.ends_with(&format!("event = \"SessionEnd\"\ncommand = {}\ntimeout = 5\n# agent-monitor hooks kimi end\n", cmd("SessionEnd"))), "{block}");
    assert_eq!(block.matches("[[hooks]]").count(), 11);
    assert!(env.ok(&["hooks", "status"]).stdout.contains("Kimi hooks: installed\n"));

    // Reinstalling is byte-for-byte the same; uninstall gives the user's file back.
    env.ok(&["hooks", "install", "--agent", "kimi"]);
    assert_eq!(std::fs::read_to_string(&config).unwrap(), text);
    assert_eq!(env.ok(&["hooks", "uninstall", "--agent", "kimi"]).stdout, "Hooks removed from ~/.kimi-code/config.toml\n");
    assert_eq!(std::fs::read_to_string(&config).unwrap(), user);

    // The legacy Kimi CLI (≤ 1.49, ~/.kimi) rejects these events at startup:
    // its config is never written, and it alone doesn't make Kimi detected.
    let tmp = Env::new();
    let legacy = tmp.home().join(".kimi/config.toml");
    std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
    std::fs::write(&legacy, "").unwrap();
    assert_eq!(tmp.ok(&["hooks", "install"]).stdout, "Hooks installed into ~/.claude/settings.json\n");
    assert_eq!(tmp.ok(&["hooks", "install", "--agent", "kimi"]).stdout, "Hooks installed into ~/.kimi-code/config.toml\n");
    assert_eq!(std::fs::read_to_string(&legacy).unwrap(), "");
}

/// Hermes Agent: a YAML block in config.yaml plus approvals in its
/// shell-hook allowlist, both undone by the uninstall.
#[test]
fn hooks_for_hermes() {
    let mut env = Env::new();
    let fake_hook = env.bin().join("my-am-hook");
    std::fs::write(&fake_hook, "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&fake_hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    env.extra_env.push(("AM_HOOK_BIN".into(), fake_hook.display().to_string()));
    let config = env.home().join(".hermes/config.yaml");
    let allowlist = env.home().join(".hermes/shell-hooks-allowlist.json");
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    let cmd = |e: &str| format!("\"{}\" hook-event --agent hermes --worktree \"\" --event {e}", fake_hook.display());

    // No hooks key yet: the block (with its own `hooks:`) is appended.
    let user = "model:\n  default: anthropic/claude-sonnet\n";
    std::fs::write(&config, user).unwrap();
    assert_eq!(env.ok(&["hooks", "install", "--agent", "hermes"]).stdout, "Hooks installed into ~/.hermes/config.yaml\n");
    let text = std::fs::read_to_string(&config).unwrap();
    let quoted = serde_json::to_string(&cmd("on_session_end")).unwrap();
    assert!(text.starts_with(&format!("{user}\n# agent-monitor hooks hermes begin\nhooks:\n  on_session_start:\n")), "{text}");
    assert!(text.contains(&format!("  on_session_end:\n    - command: {quoted}\n      timeout: 5\n")), "{text}");
    let approvals: Value = serde_json::from_str(&std::fs::read_to_string(&allowlist).unwrap()).unwrap();
    let pairs: Vec<(&str, &str)> =
        approvals["approvals"].as_array().unwrap().iter().map(|a| (a["event"].as_str().unwrap(), a["command"].as_str().unwrap())).collect();
    assert_eq!(pairs.len(), 9);
    assert!(pairs.contains(&("on_session_end", cmd("on_session_end").as_str())), "{pairs:?}");
    assert_eq!(env.ok(&["hooks", "status", "--json"]).json()["agents"]["hermes"], true);

    // Reinstalling changes neither file.
    let allow_text = std::fs::read_to_string(&allowlist).unwrap();
    env.ok(&["hooks", "install", "--agent", "hermes"]);
    assert_eq!(std::fs::read_to_string(&config).unwrap(), text);
    assert_eq!(std::fs::read_to_string(&allowlist).unwrap(), allow_text);

    // Uninstall restores the config and removes am's approvals.
    env.ok(&["hooks", "uninstall", "--agent", "hermes"]);
    assert_eq!(std::fs::read_to_string(&config).unwrap(), user);
    assert!(!allowlist.exists());
    assert_eq!(env.ok(&["hooks", "status", "--json"]).json()["agents"]["hermes"], false);

    // The user's own `hooks:` key: am's block goes inside it, and comes out again.
    let mine = "hooks:\n  subagent_stop:\n    - command: ~/.hermes/agent-hooks/log.sh\nhooks_auto_accept: false\n";
    std::fs::write(&config, mine).unwrap();
    env.ok(&["hooks", "install", "--agent", "hermes"]);
    let text = std::fs::read_to_string(&config).unwrap();
    assert!(text.starts_with("hooks:\n  # agent-monitor hooks hermes begin\n  on_session_start:\n    - command: "), "{text}");
    assert_eq!(text.matches("hooks:\n").count(), 1, "{text}");
    env.ok(&["hooks", "install", "--agent", "hermes"]);
    assert_eq!(std::fs::read_to_string(&config).unwrap(), text, "reinstalling changes nothing");
    env.ok(&["hooks", "uninstall", "--agent", "hermes"]);
    assert_eq!(std::fs::read_to_string(&config).unwrap(), mine);

    // A user's `pre_tool_call` guard stays; am's block leaves that event out.
    let guard = "hooks:\n  pre_tool_call:\n    - command: ~/.hermes/agent-hooks/guard.sh\n";
    std::fs::write(&config, guard).unwrap();
    env.ok(&["hooks", "install", "--agent", "hermes"]);
    let text = std::fs::read_to_string(&config).unwrap();
    assert_eq!(text.matches("pre_tool_call:").count(), 1, "{text}");
    assert!(text.contains("--event on_session_end"), "{text}");
    env.ok(&["hooks", "uninstall", "--agent", "hermes"]);
    assert_eq!(std::fs::read_to_string(&config).unwrap(), guard);

    // `hooks: {}` is refused before either file is written.
    std::fs::write(&config, "hooks: {}\n").unwrap();
    let o = env.run(&["hooks", "install", "--agent", "hermes"]);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("sets `hooks: {}` on one line"), "{}", o.stderr);
    assert_eq!(std::fs::read_to_string(&config).unwrap(), "hooks: {}\n");
    assert!(!allowlist.exists());

    // $HERMES_HOME moves both files.
    let other = env.root().join("hermes-home");
    env.extra_env.push(("HERMES_HOME".into(), other.display().to_string()));
    env.ok(&["hooks", "install", "--agent", "hermes"]);
    assert!(std::fs::read_to_string(other.join("config.yaml")).unwrap().contains("--event pre_llm_call"));
    assert!(other.join("shell-hooks-allowlist.json").exists());
}

/// `$HERMES_HOME` without `~/.hermes` still counts as Hermes being there,
/// so a bare `am hooks install` (every detected agent) includes it.
#[test]
fn hermes_is_detected_from_hermes_home() {
    let mut env = Env::new();
    let home = env.root().join("hermes-home");
    std::fs::create_dir_all(&home).unwrap();
    env.extra_env.push(("HERMES_HOME".into(), home.display().to_string()));
    env.ok(&["hooks", "install"]);
    assert!(std::fs::read_to_string(home.join("config.yaml")).unwrap().contains("--event pre_llm_call"));
    assert!(!env.home().join(".hermes").exists());
    assert_eq!(env.ok(&["hooks", "status", "--json"]).json()["agents"]["hermes"], true);
}

/// OpenCode has no hooks: am writes (and owns) a plugin file it auto-loads.
#[test]
fn hooks_for_opencode_are_a_plugin_file() {
    let mut env = Env::new();
    let fake_hook = env.bin().join("my-am-hook");
    std::fs::write(&fake_hook, "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&fake_hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    env.extra_env.push(("AM_HOOK_BIN".into(), fake_hook.display().to_string()));

    let plugin = env.home().join(".config/opencode/plugins/agent-monitor.js");
    assert_eq!(
        env.ok(&["hooks", "install", "--agent", "opencode"]).stdout,
        "Hooks installed into ~/.config/opencode/plugins/agent-monitor.js\n"
    );
    let text = std::fs::read_to_string(&plugin).unwrap();
    assert!(text.contains(am_core::hooks_installer::OWNED_FILE_MARKER));
    let argv = format!("const HOOK = [\"{}\", \"hook-event\", \"--agent\", \"opencode\", \"--worktree\", \"\", \"--event\"];", fake_hook.display());
    assert!(text.contains(&argv), "{text}");
    // Nothing to register: OpenCode loads everything in plugins/.
    assert!(!env.home().join(".config/opencode/opencode.json").exists());
    assert_eq!(env.ok(&["hooks", "status", "--json"]).json()["agents"]["opencode"], true);
    env.ok(&["hooks", "install", "--agent", "opencode"]);
    assert_eq!(std::fs::read_to_string(&plugin).unwrap(), text, "reinstalling changes nothing");
    let doctor = env.ok(&["doctor"]).stdout;
    assert!(doctor.contains("OpenCode hooks: Installed in ~/.config/opencode/plugins/agent-monitor.js"), "{doctor}");

    // A binary that's gone is found in the plugin.
    std::fs::remove_file(&fake_hook).unwrap();
    let doctor = env.ok(&["doctor"]).stdout;
    assert!(doctor.contains(&format!("run a missing {}", fake_hook.display())), "{doctor}");

    env.ok(&["hooks", "uninstall", "--agent", "opencode"]);
    assert!(!plugin.exists());
    assert_eq!(env.ok(&["hooks", "status", "--json"]).json()["agents"]["opencode"], false);

    // A file of that name that isn't am's is left alone.
    std::fs::write(&plugin, "export const Mine = async () => ({});\n").unwrap();
    let o = env.run(&["hooks", "install", "--agent", "opencode"]);
    assert_eq!(o.code, 1, "{}", o.stdout);
    env.ok(&["hooks", "uninstall", "--agent", "opencode"]);
    assert_eq!(std::fs::read_to_string(&plugin).unwrap(), "export const Mine = async () => ({});\n");

    // OpenCode's config dir overrides.
    let xdg = env.root().join("xdg");
    env.extra_env.push(("XDG_CONFIG_HOME".into(), xdg.display().to_string()));
    env.ok(&["hooks", "install", "--agent", "opencode"]);
    assert!(xdg.join("opencode/plugins/agent-monitor.js").exists());
    // $OPENCODE_CONFIG_DIR is never written: OpenCode loads it beside the
    // global dir, so a second copy there would run every hook twice.
    let own = env.root().join("oc");
    env.extra_env.push(("OPENCODE_CONFIG_DIR".into(), own.display().to_string()));
    env.ok(&["hooks", "install", "--agent", "opencode"]);
    assert!(!own.join("plugins/agent-monitor.js").exists());
    assert_eq!(env.ok(&["hooks", "status", "--json"]).json()["agents"]["opencode"], true);
}

/// OpenCode under `$XDG_CONFIG_HOME` (no `~/.config/opencode`) is detected,
/// so a default `am hooks install` covers it.
#[test]
fn opencode_in_an_xdg_config_dir_is_detected() {
    let mut env = Env::new();
    let fake_hook = env.bin().join("am-hook");
    std::fs::write(&fake_hook, "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&fake_hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    env.extra_env.push(("AM_HOOK_BIN".into(), fake_hook.display().to_string()));
    let xdg = env.root().join("xdg");
    std::fs::create_dir_all(xdg.join("opencode")).unwrap();
    env.extra_env.push(("XDG_CONFIG_HOME".into(), xdg.display().to_string()));
    assert!(!env.home().join(".config/opencode").exists());
    env.ok(&["hooks", "install"]);
    assert!(xdg.join("opencode/plugins/agent-monitor.js").exists());
    assert_eq!(env.ok(&["hooks", "status", "--json"]).json()["agents"]["opencode"], true);
}

/// Pi has no hook config: am owns a generated extension in Pi's agent dir.
#[test]
fn hooks_for_pi_write_an_owned_extension() {
    let mut env = Env::new();
    let fake_hook = env.bin().join("my-am-hook");
    std::fs::write(&fake_hook, "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&fake_hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    env.extra_env.push(("AM_HOOK_BIN".into(), fake_hook.display().to_string()));

    // ~/.pi: Pi is detected and a default install covers it.
    let ext_dir = env.home().join(".pi/agent/extensions");
    std::fs::create_dir_all(&ext_dir).unwrap();
    std::fs::write(ext_dir.join("mine.ts"), "export default function () {}\n").unwrap();
    assert_eq!(
        env.ok(&["hooks", "install"]).stdout,
        "Hooks installed into ~/.claude/settings.json\nHooks installed into ~/.pi/agent/extensions/agent-monitor.ts\n"
    );
    let ext = ext_dir.join("agent-monitor.ts");
    let text = std::fs::read_to_string(&ext).unwrap();
    assert!(text.contains("agent-monitor-hook-plugin v1"), "{text}");
    let argv = json!([fake_hook.display().to_string(), "hook-event", "--agent", "pi", "--worktree", "", "--event"]);
    assert!(text.contains(&format!("const ARGV = {};", argv.to_string().replace(",", ", "))), "{text}");
    assert_eq!(env.ok(&["hooks", "status", "--json"]).json()["agents"], hooks_installed(&["claude", "pi"]));
    let doctor = env.ok(&["doctor"]).stdout;
    assert!(doctor.contains("Pi hooks: Installed in ~/.pi/agent/extensions/agent-monitor.ts"), "{doctor}");

    // Reinstalling is idempotent; uninstalling removes only am's file.
    env.ok(&["hooks", "install", "--agent", "pi"]);
    assert_eq!(std::fs::read_to_string(&ext).unwrap(), text);
    assert_eq!(
        env.ok(&["hooks", "uninstall", "--agent", "pi"]).stdout,
        "Hooks removed from ~/.pi/agent/extensions/agent-monitor.ts\n"
    );
    assert!(!ext.exists());
    assert_eq!(std::fs::read_to_string(ext_dir.join("mine.ts")).unwrap(), "export default function () {}\n");

    // A file of that name am didn't write is left alone.
    std::fs::write(&ext, "// my own\n").unwrap();
    let o = env.run(&["hooks", "install", "--agent", "pi"]);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("wasn't written by am"), "{}", o.stderr);
    assert_eq!(std::fs::read_to_string(&ext).unwrap(), "// my own\n");
    std::fs::remove_file(&ext).unwrap();

    // $PI_CODING_AGENT_DIR moves the agent dir, extensions with it.
    let agent_dir = env.root().join("pi-agent");
    env.extra_env.push(("PI_CODING_AGENT_DIR".into(), agent_dir.display().to_string()));
    env.ok(&["hooks", "install", "--agent", "pi"]);
    let moved = agent_dir.join("extensions/agent-monitor.ts");
    assert!(std::fs::read_to_string(&moved).unwrap().contains("agent-monitor-hook-plugin v1"));
    assert!(!ext.exists());

    // A deleted am-hook is reported by doctor.
    std::fs::remove_file(&fake_hook).unwrap();
    let doctor = env.ok(&["doctor"]).stdout;
    assert!(
        doctor.contains(&format!("[WARN]  Pi hooks: Hooks in {} run a missing {}", moved.display(), fake_hook.display())),
        "{doctor}"
    );
    env.ok(&["hooks", "uninstall", "--agent", "pi"]);
    assert!(!moved.exists());
}

/// Built-in harness ids in registry order, so adding a harness doesn't
/// touch these tests.
fn builtin_ids() -> Vec<&'static str> {
    am_core::harness::BUILTIN.iter().map(|k| k.id()).collect()
}

/// `hooks status --json`'s `agents` map: every built-in, `true` for `on`.
fn hooks_installed(on: &[&str]) -> Value {
    Value::Object(builtin_ids().into_iter().map(|id| (id.to_string(), Value::Bool(on.contains(&id)))).collect())
}

#[test]
fn hooks_for_qoder() {
    let mut env = Env::new();
    let fake_hook = env.bin().join("my-am-hook");
    std::fs::write(&fake_hook, "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&fake_hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    env.extra_env.push(("AM_HOOK_BIN".into(), fake_hook.display().to_string()));

    // Qoder CLI: Claude-style nested entries in ~/.qoder/settings.json, timeout in seconds.
    let qoder_settings = env.home().join(".qoder/settings.json");
    std::fs::create_dir_all(qoder_settings.parent().unwrap()).unwrap();
    std::fs::write(&qoder_settings, json!({"model": "auto"}).to_string()).unwrap();
    assert_eq!(env.ok(&["hooks", "install", "--agent", "qoder"]).stdout, "Hooks installed into ~/.qoder/settings.json\n");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&qoder_settings).unwrap()).unwrap();
    assert_eq!(v["model"], "auto");
    assert_eq!(v["hooks"].as_object().unwrap().len(), 11);
    let stop = &v["hooks"]["Stop"][0]["hooks"][0];
    assert_eq!((stop["type"].as_str(), stop["timeout"].as_u64()), (Some("command"), Some(5)));
    assert_eq!(
        stop["command"],
        format!("\"{}\" hook-event --agent qoder --worktree \"$QODER_PROJECT_DIR\" --event Stop", fake_hook.display())
    );
    assert_eq!(env.ok(&["hooks", "status", "--json"]).json()["agents"]["qoder"], true);
    assert_eq!(env.ok(&["hooks", "uninstall", "--agent", "qoder"]).stdout, "Hooks removed from ~/.qoder/settings.json\n");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&qoder_settings).unwrap()).unwrap();
    assert_eq!(v, json!({"model": "auto"}));
}

/// With nothing to resume, `am agent attach` suggests the default agent's
/// fresh-start command, `resume.start` included (`acli rovodev run`).
#[test]
fn agent_attach_suggests_the_fresh_start_command() {
    let env = Env::new();
    let (_, wt) = env.seed_repo();
    std::fs::create_dir_all(env.data().join("agents")).unwrap();
    std::fs::write(
        env.data().join("agents/mycli.json"),
        json!({
            "id": "mycli", "name": "My CLI", "program": "mycli",
            "hooks": {"settingsPath": "~/.mycli/hooks.json"},
            "events": {"Done": "Stop"},
            "resume": {"session": ["--resume", "{id}"], "start": ["chat", "--agent", "am"]}
        })
        .to_string(),
    )
    .unwrap();
    env.ok(&["settings", "set", "defaultAgent", "mycli"]);
    let o = env.run(&["agent", "attach", "feature/login"]);
    assert_eq!(o.code, 1, "{}", o.stdout);
    assert!(o.stderr.contains(&format!("Start one with: cd {} && mycli chat --agent am", wt.display())), "{}", o.stderr);
}

#[test]
fn hooks_for_codebuddy() {
    let mut env = Env::new();
    let fake_hook = env.bin().join("my-am-hook");
    std::fs::write(&fake_hook, "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&fake_hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    env.extra_env.push(("AM_HOOK_BIN".into(), fake_hook.display().to_string()));

    // CodeBuddy keeps Claude's nested layout in ~/.codebuddy/settings.json, timeout in seconds.
    let settings = env.home().join(".codebuddy/settings.json");
    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
    let mine = json!({"matcher": "Bash", "hooks": [{"type": "command", "command": "./guard.sh"}]});
    std::fs::write(&settings, json!({"model": "x", "hooks": {"PreToolUse": [mine]}}).to_string()).unwrap();
    assert_eq!(env.ok(&["hooks", "install", "--agent", "codebuddy"]).stdout, "Hooks installed into ~/.codebuddy/settings.json\n");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    assert_eq!(v["model"], "x");
    let hooks = v["hooks"].as_object().unwrap();
    assert_eq!(
        hooks.keys().map(String::as_str).collect::<Vec<_>>(),
        ["PreToolUse", "SessionStart", "SessionEnd", "UserPromptSubmit", "PostToolUse", "PermissionRequest", "Notification", "SubagentStart", "SubagentStop", "Stop", "StopFailure"]
    );
    let pre = hooks["PreToolUse"].as_array().unwrap();
    assert_eq!(pre[0], mine);
    let entry = &pre[1]["hooks"][0];
    assert_eq!(entry["timeout"], 5);
    assert_eq!(
        entry["command"],
        format!("\"{}\" hook-event --agent codebuddy --worktree \"$CODEBUDDY_PROJECT_DIR\" --event PreToolUse", fake_hook.display())
    );
    assert_eq!(env.ok(&["hooks", "status", "--json"]).json()["agents"]["codebuddy"], true);

    assert_eq!(env.ok(&["hooks", "uninstall", "--agent", "codebuddy"]).stdout, "Hooks removed from ~/.codebuddy/settings.json\n");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    assert_eq!(v, json!({"model": "x", "hooks": {"PreToolUse": [mine]}}));

    // $CODEBUDDY_CONFIG_DIR moves CodeBuddy's config, and the hooks with it:
    // installed there (detected from it alone), never into ~/.codebuddy.
    std::fs::remove_dir_all(env.home().join(".codebuddy")).unwrap();
    let moved = env.root().join("cb-config");
    std::fs::create_dir_all(&moved).unwrap();
    env.extra_env.push(("CODEBUDDY_CONFIG_DIR".into(), moved.display().to_string()));
    assert_eq!(env.ok(&["hooks", "status", "--json"]).json()["agents"]["codebuddy"], false);
    env.ok(&["hooks", "install"]);
    let v: Value = serde_json::from_str(&std::fs::read_to_string(moved.join("settings.json")).unwrap()).unwrap();
    assert_eq!(v["hooks"].as_object().unwrap().len(), 11);
    assert!(!env.home().join(".codebuddy").exists());
    assert_eq!(env.ok(&["hooks", "status", "--json"]).json()["agents"]["codebuddy"], true);
}

/// Antigravity's hook file holds named groups; am owns `agent-monitor` and
/// leaves the others (cmux's) alone.
#[test]
fn hooks_for_antigravity() {
    let mut env = Env::new();
    let fake_hook = env.bin().join("my-am-hook");
    std::fs::write(&fake_hook, "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&fake_hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    env.extra_env.push(("AM_HOOK_BIN".into(), fake_hook.display().to_string()));

    // Gemini CLI's dir alone doesn't make Antigravity detected.
    std::fs::create_dir_all(env.home().join(".gemini")).unwrap();
    assert!(!env.ok(&["hooks", "install"]).stdout.contains("config/hooks.json"));

    let file = env.home().join(".gemini/config/hooks.json");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    let cmux = json!({"Stop": [{"type": "command", "command": "cmux hooks antigravity stop", "timeout": 10}]});
    std::fs::write(&file, json!({"cmux": cmux}).to_string()).unwrap();
    assert_eq!(env.ok(&["hooks", "install", "--agent", "antigravity"]).stdout, "Hooks installed into ~/.gemini/config/hooks.json\n");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(v["cmux"], cmux);
    let ours = v["agent-monitor"].as_object().unwrap();
    assert_eq!(ours.keys().map(String::as_str).collect::<Vec<_>>(), ["SessionStart", "PreInvocation", "Stop"]);
    assert_eq!(
        ours["Stop"],
        json!([{"type": "command", "command": format!("\"{}\" hook-event --agent antigravity --worktree \"\" --event Stop", fake_hook.display()), "timeout": 10}])
    );
    // Idempotent.
    env.ok(&["hooks", "install", "--agent", "antigravity"]);
    assert_eq!(serde_json::from_str::<Value>(&std::fs::read_to_string(&file).unwrap()).unwrap(), v);
    assert_eq!(env.ok(&["hooks", "status", "--json"]).json()["agents"]["antigravity"], true);

    // Uninstall drops our group and keeps cmux's.
    env.ok(&["hooks", "uninstall", "--agent", "antigravity"]);
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(v, json!({"cmux": cmux}));

    // Once agy has run here, a default install covers it.
    std::fs::create_dir_all(env.home().join(".gemini/antigravity-cli")).unwrap();
    assert!(env.ok(&["hooks", "install"]).stdout.contains("Hooks installed into ~/.gemini/config/hooks.json"));
}

#[test]
fn hooks_for_grok_go_in_an_owned_file() {
    let mut env = Env::new();
    let fake_hook = env.bin().join("my-am-hook");
    std::fs::write(&fake_hook, "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&fake_hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    env.extra_env.push(("AM_HOOK_BIN".into(), fake_hook.display().to_string()));

    // Grok reads every file in ~/.grok/hooks/: am's own sits beside the user's.
    let hooks_dir = env.home().join(".grok/hooks");
    std::fs::create_dir_all(&hooks_dir).unwrap();
    std::fs::write(hooks_dir.join("mine.json"), "{}").unwrap();
    assert_eq!(env.ok(&["hooks", "install", "--agent", "grok"]).stdout, "Hooks installed into ~/.grok/hooks/agent-monitor.json\n");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(hooks_dir.join("agent-monitor.json")).unwrap()).unwrap();
    let hooks = v["hooks"].as_object().unwrap();
    assert_eq!(hooks.keys().map(String::as_str).collect::<Vec<_>>(), ["SessionStart", "UserPromptSubmit", "PreToolUse", "Notification", "Stop"]);
    assert_eq!(
        hooks["Stop"],
        json!([{"hooks": [{"type": "command", "command": format!("\"{}\" hook-event --agent grok --worktree \"\" --event Stop", fake_hook.display()), "timeout": 5}]}])
    );
    assert_eq!(std::fs::read_to_string(hooks_dir.join("mine.json")).unwrap(), "{}");
    assert_eq!(env.ok(&["hooks", "status", "--json"]).json()["agents"]["grok"], true);
    // Installed by default now that ~/.grok exists, and idempotent.
    env.ok(&["hooks", "install"]);
    let again: Value = serde_json::from_str(&std::fs::read_to_string(hooks_dir.join("agent-monitor.json")).unwrap()).unwrap();
    assert_eq!(again, v);

    env.ok(&["hooks", "uninstall", "--agent", "grok"]);
    assert_eq!(env.ok(&["hooks", "status", "--json"]).json()["agents"]["grok"], false);
}

/// Muse Code: hooks in `$XDG_CONFIG_HOME/muse/settings.json` (else
/// `~/.config/muse/`), seconds timeouts, and the `schema_version` Muse needs.
#[test]
fn hooks_for_muse() {
    let mut env = Env::new();
    let fake_hook = env.bin().join("my-am-hook");
    std::fs::write(&fake_hook, "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&fake_hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    env.extra_env.push(("AM_HOOK_BIN".into(), fake_hook.display().to_string()));

    // A fresh install creates the file with the schema version Muse requires.
    assert_eq!(env.ok(&["hooks", "install", "--agent", "muse"]).stdout, "Hooks installed into ~/.config/muse/settings.json\n");
    let fresh = env.home().join(".config/muse/settings.json");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&fresh).unwrap()).unwrap();
    assert_eq!(v["schema_version"], 1);
    let stop = &v["hooks"]["Stop"][0]["hooks"][0];
    assert_eq!(stop["timeout"], 10);
    assert_eq!(stop["command"], format!("\"{}\" hook-event --agent muse --worktree \"\" --event Stop", fake_hook.display()));
    assert_eq!(
        v["hooks"].as_object().unwrap().keys().map(String::as_str).collect::<Vec<_>>(),
        ["SessionStart", "SessionEnd", "UserPromptSubmit", "Notification", "Stop", "StopFailure"]
    );

    // With $XDG_CONFIG_HOME, that's where Muse reads; its other keys, its own
    // schema version and the user's hooks stay.
    let xdg = env.root().join("xdg");
    env.extra_env.push(("XDG_CONFIG_HOME".into(), xdg.display().to_string()));
    let settings = xdg.join("muse/settings.json");
    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
    let mine = json!({"hooks": [{"type": "command", "command": "./notify.sh"}]});
    std::fs::write(&settings, json!({"schema_version": 2, "model": "m", "hooks": {"Stop": [mine]}}).to_string()).unwrap();
    // Detected through the XDG directory, so a plain install covers it.
    let out = env.ok(&["hooks", "install"]).stdout;
    assert!(out.contains(&format!("Hooks installed into {}\n", settings.display())), "{out}");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    assert_eq!((v["schema_version"].as_u64(), v["model"].as_str()), (Some(2), Some("m")));
    assert_eq!(v["hooks"]["Stop"][0], mine);
    assert_eq!(v["hooks"]["Stop"].as_array().unwrap().len(), 2);
    assert_eq!(env.ok(&["hooks", "status", "--json"]).json()["agents"]["muse"], true);

    env.ok(&["hooks", "uninstall", "--agent", "muse"]);
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    assert_eq!(v, json!({"schema_version": 2, "model": "m", "hooks": {"Stop": [mine]}}));
}

/// Kiro keeps hooks in an agent file: am writes a whole `agent-monitor`
/// agent (with every tool, or the tool hooks never fire), and keeps it.
#[test]
fn hooks_for_kiro() {
    let mut env = Env::new();
    let fake_hook = env.bin().join("my-am-hook");
    std::fs::write(&fake_hook, "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&fake_hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    env.extra_env.push(("AM_HOOK_BIN".into(), fake_hook.display().to_string()));

    // A fresh ~/.kiro (no agents dir yet): install creates the agent file.
    std::fs::create_dir_all(env.home().join(".kiro")).unwrap();
    assert_eq!(env.ok(&["hooks", "install", "--agent", "kiro"]).stdout, "Hooks installed into ~/.kiro/agents/agent-monitor.json\n");
    let file = env.home().join(".kiro/agents/agent-monitor.json");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!((&v["name"], &v["tools"]), (&json!("agent-monitor"), &json!(["*"])));
    assert!(v["description"].is_string());
    let hooks = v["hooks"].as_object().unwrap();
    assert_eq!(hooks.keys().map(String::as_str).collect::<Vec<_>>(), ["agentSpawn", "userPromptSubmit", "preToolUse", "postToolUse", "stop"]);
    assert_eq!(
        hooks["stop"],
        json!([{"command": format!("\"{}\" hook-event --agent kiro --worktree \"\" --event stop", fake_hook.display()), "timeout_ms": 5000}])
    );
    assert_eq!(env.ok(&["hooks", "status", "--json"]).json()["agents"]["kiro"], true);

    // The user's own edits to the agent survive a reinstall.
    let mut edited = v.clone();
    edited["tools"] = json!(["read", "shell"]);
    edited["hooks"]["stop"].as_array_mut().unwrap().insert(0, json!({"command": "./notify.sh"}));
    std::fs::write(&file, edited.to_string()).unwrap();
    env.ok(&["hooks", "install", "--agent", "kiro"]);
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(v["tools"], json!(["read", "shell"]));
    assert_eq!(v["hooks"]["stop"].as_array().unwrap().len(), 2);
    assert_eq!(v["hooks"]["stop"][0]["command"], "./notify.sh");

    // Uninstall takes only am's hooks out.
    assert_eq!(env.ok(&["hooks", "uninstall", "--agent", "kiro"]).stdout, "Hooks removed from ~/.kiro/agents/agent-monitor.json\n");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(v["hooks"], json!({"stop": [{"command": "./notify.sh"}]}));
    assert_eq!(env.ok(&["hooks", "status", "--json"]).json()["agents"]["kiro"], false);
}

/// A JSON manifest in `<data>/agents/` is a full harness plugin: no code.
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
    assert!(o.stderr.contains(&format!("Available: {}, mycli", builtin_ids().join(", "))), "{}", o.stderr);

    // Doctor reports the plugin's hooks and the file it skipped.
    let doctor = env.ok(&["doctor"]).stdout;
    assert!(doctor.contains("MyCLI hooks: Installed in ~/.mycli/hooks.json"), "{doctor}");
    assert!(doctor.contains("Harness plugins: Skipped in") && doctor.contains("broken.json"), "{doctor}");

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

#[test]
fn worktree_open_runs_a_custom_command() {
    let env = Env::new();
    let (repo, wt) = env.seed_repo();
    let log = env.root().join("opened.txt");
    env.fake_bin("my-editor", &format!("echo \"$@\" >> {}", log.display()));
    env.ok(&["settings", "set", "ide", "custom"]);
    env.ok(&["settings", "set", "ideCommand", "my-editor --new {path}"]);
    let o = env.ok(&["open", "feature/login", "--repo", repo.to_str().unwrap()]);
    assert_eq!(o.stdout, "Opened feature/login in custom\n");
    assert_eq!(std::fs::read_to_string(&log).unwrap().trim(), format!("--new {}", wt.display()));

    env.ok(&["settings", "set", "ideCommand", ""]);
    let o = env.run(&["open", "feature/login", "--repo", repo.to_str().unwrap()]);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("Editor command"), "{}", o.stderr);
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
    env.fake_bin("code", &format!("echo \"$@\" >> {}", log.display()));
    assert_eq!(env.ok(&["script", "edit", "--repo", repo_s]).stdout, "Opened startup script for myrepo in vscode\n");
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

#[test]
fn review_scripts_sit_beside_the_startup_script() {
    let env = Env::new();
    let (repo, _) = env.seed_repo();
    let repo_s = repo.to_str().unwrap();
    assert_eq!(env.ok(&["script", "show", "--review", "--repo", repo_s]).stdout, "No review script for myrepo\n");
    env.fake_bin("code", "true");
    assert_eq!(env.ok(&["script", "edit", "--review", "--repo", repo_s]).stdout, "Opened review script for myrepo in vscode\n");
    let path = env.data().join("scripts/r1.review.sh");
    assert!(path.exists());
    assert!(!env.data().join("scripts/r1.sh").exists(), "the startup script is untouched");
    let out = env.ok(&["script", "show", "--review", "--repo", repo_s]).stdout;
    assert!(out.starts_with("# Review script for myrepo\n# Path: "), "{out}");
    assert!(out.contains("AM_PR_NUMBER"));
    assert_eq!(env.ok(&["script", "show", "--repo", repo_s]).stdout, "No startup script for myrepo\n");
    assert_eq!(env.ok(&["script", "remove", "--review", "--repo", repo_s]).stdout, "Removed review script for myrepo\n");
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

#[test]
fn perf_reports_stages_against_budgets() {
    let env = Env::new();
    std::fs::write(
        env.data().join("debug.log"),
        "[2000-01-01T00:00:00.000Z] [INFO] [perf] ui.frame n=40 p50=1.00ms p95=3.00ms max=9.00ms budget=16.00ms\n\
         [2000-01-01T00:00:01.000Z] [DEBUG] [tui] git refresh for 56 worktree(s) in 8.1s\n\
         [2000-01-01T00:00:02.000Z] [WARN] [perf] collector.git slow: p95 8.10s > budget 3.00s (n=20, max 8.10s)\n",
    )
    .unwrap();
    // Everything is older than the default 24 h window.
    assert!(env.ok(&["perf"]).stdout.starts_with("No timings in "));
    let table = env.ok(&["perf", "--since", "all"]).stdout;
    assert!(table.contains("Over budget: collector.git"), "{table}");
    let rows = env.ok(&["perf", "--since", "all", "--json"]).json();
    let row = |stage: &str| rows.as_array().unwrap().iter().find(|r| r["stage"] == stage).cloned().unwrap();
    assert_eq!(row("ui.frame")["status"], "ok");
    assert_eq!(row("ui.frame")["samples"], 40);
    assert_eq!(row("collector.git")["status"], "SLOW");
    assert_eq!(row("collector.git")["slowWarnings"], 1);
    assert_eq!(env.run(&["perf", "--since", "1w"]).code, 1);
}

#[test]
fn logs_open_edits_the_log_or_reveals_the_folder() {
    let env = Env::new();
    assert!(env.ok(&["logs", "--open"]).stdout.starts_with("No log file found at "));
    let log = env.data().join("debug.log");
    std::fs::write(&log, "[t1] [INFO] [git] one\n").unwrap();
    let opened = env.root().join("opened.txt");
    env.fake_bin("my-editor", &format!("echo \"$@\" >> {}", opened.display()));
    env.ok(&["settings", "set", "ide", "custom"]);
    env.ok(&["settings", "set", "ideCommand", "my-editor {path}"]);
    assert_eq!(env.ok(&["logs", "--open"]).stdout, format!("Opened {}\n", log.display()));
    assert_eq!(std::fs::read_to_string(&opened).unwrap().trim(), log.to_str().unwrap());

    // Rotated: the folder opens instead (fake `open`/`xdg-open` record their args).
    std::fs::write(env.data().join("debug.log.1"), "old\n").unwrap();
    let revealed = env.root().join("revealed.txt");
    for opener in ["open", "xdg-open"] {
        env.fake_bin(opener, &format!("echo \"$@\" >> {}", revealed.display()));
    }
    assert!(env.ok(&["logs", "--open"]).stdout.starts_with("Opened the log folder "));
    let args = std::fs::read_to_string(&revealed).unwrap();
    let want = if cfg!(target_os = "macos") { format!("-R {}", log.display()) } else { env.data().display().to_string() };
    assert_eq!(args.trim(), want);
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
    git(&repo, &["push", "-q", "origin", "main:refs/heads/bob/pr"]);
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
    assert_eq!(o.json()["tmpSpace"], false);

    // Tmp space: existing branches only, set at create, listed last with `tmpSpace`.
    let o = env.run(&["worktree", "create", "alice/review", "--repo", r, "--tmp"]);
    assert!(o.code == 1 && o.stderr.contains("only takes existing branches"), "{}", o.stderr);
    git(&repo, &["branch", "alice/review"]);
    let o = env.ok(&["worktree", "create", "alice/review", "--repo", r, "--tmp", "--json"]);
    assert_eq!((o.json()["mode"].as_str(), o.json()["tmpSpace"].as_bool()), (Some("reuse"), Some(true)));
    let rows = env.ok(&["ls", "--repo", r, "--json"]).json();
    let last = rows.as_array().unwrap().last().unwrap().clone();
    assert_eq!((last["branch"].as_str(), last["tmpSpace"].as_bool(), last["repo"].as_str()), (Some("alice/review"), Some(true), Some("repo")));
    assert!(env.ok(&["ls", "--repo", r]).stdout.lines().next().unwrap().contains("Space"));
    assert_eq!(env.run(&["worktree", "tmp", "alice/review", "--repo", r]).code, 1, "moving in/out is gone");
    env.ok(&["worktree", "delete", "alice/review", "--repo", r, "--delete-branch", "--force"]);
    // --tmp pulls a branch that exists on origin without asking for --track.
    let o = env.ok(&["worktree", "create", "bob/pr", "--repo", r, "--tmp", "--json"]);
    assert_eq!((o.json()["mode"].as_str(), o.json()["tmpSpace"].as_bool()), (Some("track"), Some(true)));
    assert!(env.ok(&["worktree", "delete", "bob/pr", "--repo", r, "--delete-branch", "--force"]).stdout.contains("Deleted worktree: "));
    let o = env.ok(&["worktree", "create", "bob/pr", "--repo", r, "--tmp"]);
    assert!(o.stdout.contains("Branch: bob/pr (tracking origin/bob/pr)"), "{}", o.stdout);
    env.ok(&["worktree", "delete", "bob/pr", "--repo", r, "--delete-branch", "--force"]);

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

// --- mcp ---

/// Send JSON-RPC messages to `am mcp` (started in `cwd`) and collect the
/// replies, one per request; notifications get none.
fn mcp_session(env: &Env, cwd: &Path, messages: &[Value]) -> Vec<Value> {
    let mut child = env.command(&["mcp"], Some(cwd)).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    for m in messages {
        writeln!(stdin, "{m}").unwrap();
    }
    writeln!(stdin, "not json").unwrap();
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    stdout
        .lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("non-protocol output on stdout ({e}): {l}")))
        .collect()
}

fn rpc(id: u64, method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

#[test]
fn mcp_server_runs_cli_commands_as_tools() {
    let env = Env::new();
    let (repo, _wt) = env.seed_repo();
    let replies = mcp_session(
        &env,
        &repo,
        &[
            rpc(1, "initialize", json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test"}})),
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            rpc(2, "tools/list", json!({})),
            rpc(3, "tools/call", json!({"name": "worktree_list", "arguments": {}})),
            rpc(4, "tools/call", json!({"name": "worktree_info", "arguments": {"target": "nope"}})),
            rpc(5, "tools/call", json!({"name": "settings_get", "arguments": {"key": "ghPrStatus"}})),
            rpc(6, "tools/call", json!({"name": "worktree_delete", "arguments": {"target": "--force"}})),
        ],
    );
    assert_eq!(replies.len(), 7, "{replies:#?}");
    assert_eq!(replies[0]["result"]["protocolVersion"], "2025-06-18");
    let tools = replies[1]["result"]["tools"].as_array().unwrap();
    assert!(tools.iter().any(|t| t["name"] == "worktree_create"));

    // Run from inside the repo: the CLI detects it from the cwd.
    let list = &replies[2]["result"];
    assert_eq!(list["isError"], false, "{list}");
    let rows = list["structuredContent"]["result"].as_array().unwrap();
    assert!(rows.iter().any(|r| r["branch"] == "feature/login"), "{rows:?}");

    // A failing command is a tool error and the server keeps going.
    assert_eq!(replies[3]["result"]["isError"], true);
    assert_eq!(replies[4]["result"]["isError"], false);
    assert_eq!(replies[5]["result"]["isError"], true);
    assert!(replies[5]["result"]["content"][0]["text"].as_str().unwrap().contains("can't start with '-'"));
    assert_eq!(replies[6]["error"]["code"], -32700);
}

#[test]
fn mcp_install_registers_with_each_agent() {
    let env = Env::new();
    std::fs::create_dir_all(env.home().join(".gemini")).unwrap();
    std::fs::write(env.home().join(".gemini/settings.json"), r#"{"mcpServers":{"other":{"command":"x"}}}"#).unwrap();

    // No `claude` on PATH: Claude Code's ~/.claude.json is edited directly.
    let o = env.ok(&["mcp", "install"]);
    assert!(o.stdout.contains("Claude Code (~/.claude.json)"), "{}", o.stdout);
    assert!(o.stdout.contains("Gemini CLI (~/.gemini/settings.json)"), "{}", o.stdout);
    let claude: Value = serde_json::from_str(&std::fs::read_to_string(env.home().join(".claude.json")).unwrap()).unwrap();
    assert_eq!(claude["mcpServers"]["agent-monitor"]["args"], json!(["mcp"]));
    assert_eq!(claude["mcpServers"]["agent-monitor"]["type"], "stdio");
    let gemini: Value =
        serde_json::from_str(&std::fs::read_to_string(env.home().join(".gemini/settings.json")).unwrap()).unwrap();
    assert_eq!(gemini["mcpServers"]["other"]["command"], "x");
    assert!(gemini["mcpServers"]["agent-monitor"]["command"].as_str().unwrap().ends_with("am"));

    let status = env.ok(&["mcp", "status", "--json"]).json();
    assert_eq!(status["agents"]["claude"]["installed"], true);
    assert_eq!(status["agents"]["gemini"]["installed"], true);
    assert_eq!(status["agents"]["codex"]["supported"], false);

    env.ok(&["mcp", "uninstall"]);
    let gemini: Value =
        serde_json::from_str(&std::fs::read_to_string(env.home().join(".gemini/settings.json")).unwrap()).unwrap();
    assert_eq!(gemini, json!({"mcpServers": {"other": {"command": "x"}}}));

    let o = env.run(&["mcp", "install", "--agent", "codex"]);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("can't register"), "{}", o.stderr);
}

#[test]
fn mcp_install_registers_with_claude_desktop() {
    let env = Env::new();
    let rel = if cfg!(target_os = "macos") { "Library/Application Support/Claude" } else { ".config/Claude" };
    let dir = env.home().join(rel);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("claude_desktop_config.json");
    std::fs::write(&file, r#"{"mcpServers":{"other":{"command":"x"}},"preferences":{"a":1}}"#).unwrap();

    // Present, so the default install picks it up; it has no CLI, so the file is edited.
    let o = env.ok(&["mcp", "install"]);
    assert!(o.stdout.contains("MCP server registered with Claude Desktop (~/"), "{}", o.stdout);
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    let entry = &v["mcpServers"]["agent-monitor"];
    assert_eq!(entry["args"], json!(["mcp"]));
    assert!(entry["env"]["PATH"].as_str().unwrap().contains("/usr/bin"), "{entry}");
    assert_eq!(v["mcpServers"]["other"]["command"], "x");
    assert_eq!(v["preferences"]["a"], 1);

    let status = env.ok(&["mcp", "status", "--json"]).json();
    assert_eq!(status["agents"]["claude-desktop"]["installed"], true);
    assert!(env.ok(&["mcp", "status"]).stdout.contains("Claude Desktop MCP server: registered"));

    env.ok(&["mcp", "uninstall", "--agent", "claude-desktop"]);
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(v, json!({"mcpServers": {"other": {"command": "x"}}, "preferences": {"a": 1}}));
}

#[test]
fn mcp_tools_outside_a_repo_use_the_only_tracked_one() {
    let env = Env::new();
    env.seed_repo();
    // Claude Desktop starts the server in `/`.
    let replies = mcp_session(
        &env,
        Path::new("/"),
        &[rpc(1, "tools/call", json!({"name": "worktree_info", "arguments": {"target": "feature/login"}}))],
    );
    assert_eq!(replies[0]["result"]["isError"], false, "{}", replies[0]);

    // The CLI itself doesn't guess, and says what is tracked.
    let o = env.run_in(Some(Path::new("/")), &["worktree", "info", "feature/login"]);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("Tracked: ") && o.stderr.contains("myrepo"), "{}", o.stderr);
}

#[test]
fn mcp_install_uses_the_claude_cli_when_present() {
    let env = Env::new();
    let log = env.root().join("claude-args");
    env.fake_bin("claude", &format!("echo \"$@\" >> '{}'", log.display()));
    // Already registered: removed first, since `claude mcp add-json` refuses an existing name.
    std::fs::write(env.home().join(".claude.json"), r#"{"mcpServers":{"agent-monitor":{"command":"old"}}}"#).unwrap();
    let o = env.ok(&["mcp", "install", "--agent", "claude"]);
    assert!(o.stdout.contains("claude mcp add-json"), "{}", o.stdout);
    let calls = std::fs::read_to_string(&log).unwrap();
    let calls: Vec<&str> = calls.lines().collect();
    assert_eq!(calls[0], "mcp remove --scope user agent-monitor");
    assert!(calls[1].starts_with("mcp add-json --scope user agent-monitor {\"type\":\"stdio\",\"command\":"), "{}", calls[1]);
}
