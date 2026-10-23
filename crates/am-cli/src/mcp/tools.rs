//! The MCP tools: one per non-interactive `am` command. Each tool validates its
//! arguments against its [`Param`]s and turns them into an `am` argv; the
//! server runs that as a child process. Commands that take over the terminal
//! or never return (the TUI, `agent attach`, `script edit`, `logs --follow`)
//! have no tool.

use std::time::Duration;

use serde_json::{Map, Value, json};

#[derive(Clone, Copy)]
pub enum Kind {
    Str,
    Bool,
    Int,
    /// A string, number or bool, passed on as text (`settings set` values).
    Scalar,
    Enum(&'static [&'static str]),
    StrList,
}

pub struct Param {
    pub name: &'static str,
    pub kind: Kind,
    pub required: bool,
    pub description: &'static str,
}

const fn req(name: &'static str, kind: Kind, description: &'static str) -> Param {
    Param { name, kind, required: true, description }
}

const fn opt(name: &'static str, kind: Kind, description: &'static str) -> Param {
    Param { name, kind, required: false, description }
}

/// MCP tool annotations, so clients can ask before the risky ones.
#[derive(Clone, Copy)]
pub struct Hints {
    pub read_only: bool,
    pub destructive: bool,
    pub idempotent: bool,
    /// Talks to GitHub/Linear or launches an app.
    pub open_world: bool,
}

const READ: Hints = Hints { read_only: true, destructive: false, idempotent: true, open_world: false };
const READ_NET: Hints = Hints { open_world: true, ..READ };
const WRITE: Hints = Hints { read_only: false, destructive: false, idempotent: false, open_world: false };
const WRITE_SAME: Hints = Hints { idempotent: true, ..WRITE };
const DESTRUCTIVE: Hints = Hints { destructive: true, ..WRITE };
const LAUNCH: Hints = Hints { open_world: true, ..WRITE };

const SHORT: Duration = Duration::from_secs(60);
/// Creating and deleting fetch and push.
const LONG: Duration = Duration::from_secs(180);

pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    pub params: &'static [Param],
    pub hints: Hints,
    pub timeout: Duration,
    build: fn(&Args) -> Vec<String>,
}

/// Validated arguments of one call.
pub struct Args<'a>(&'a Map<String, Value>);

impl Args<'_> {
    fn str(&self, k: &str) -> Option<String> {
        match self.0.get(k)? {
            Value::String(s) => Some(s.clone()),
            Value::Null => None,
            v => Some(v.to_string()),
        }
    }

    fn flag(&self, k: &str) -> bool {
        self.0.get(k).and_then(Value::as_bool).unwrap_or(false)
    }

    fn list(&self, k: &str) -> Vec<String> {
        self.0.get(k).and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(String::from).collect()
    }
}

/// `["worktree", "list"]` + `--json` etc. without `vec![...to_string()]` noise.
fn cmd(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

/// Append `flag value` when the argument is set.
fn with(mut argv: Vec<String>, a: &Args, key: &str, flag: &str) -> Vec<String> {
    if let Some(v) = a.str(key) {
        argv.push(flag.into());
        argv.push(v);
    }
    argv
}

/// Append `flag` when the boolean argument is true.
fn switch(mut argv: Vec<String>, a: &Args, key: &str, flag: &str) -> Vec<String> {
    if a.flag(key) {
        argv.push(flag.into());
    }
    argv
}

/// Append the positional argument when set.
fn pos(mut argv: Vec<String>, a: &Args, key: &str) -> Vec<String> {
    argv.extend(a.str(key));
    argv
}

fn repo(argv: Vec<String>, a: &Args) -> Vec<String> {
    with(argv, a, "repo", "--repo")
}

fn agents(mut argv: Vec<String>, a: &Args) -> Vec<String> {
    for id in a.list("agents") {
        argv.extend(["--agent".into(), id]);
    }
    argv
}

const REPO: Param = opt("repo", Kind::Str, "Repository path (default: the repo containing the agent's working directory)");
const TARGET: Param = req("target", Kind::Str, "Worktree branch name or path");
const TARGET_OPT: Param = opt("target", Kind::Str, "Worktree branch name or path (default: the current branch)");
const AGENTS: Param = opt(
    "agents",
    Kind::StrList,
    "Agent ids (claude, gemini, codex, qwen, droid, copilot, cursor, …); default: every detected agent",
);
const STATUSES: &[&str] = &["idle", "executing", "planning", "waiting", "delegating", "done"];
const MODES: &[&str] = &["fresh", "reuse", "track", "no_track"];
const LEVELS: &[&str] = &["debug", "info", "warn", "error"];

pub static TOOLS: &[Tool] = &[
    Tool {
        name: "worktree_list",
        description: "List the tracked worktrees with their agent status, git state (ahead/behind/dirty), PR + CI state and Linear ticket. Refreshes GitHub/Linear first.",
        params: &[REPO],
        hints: READ_NET,
        timeout: SHORT,
        build: |a| repo(cmd(&["worktree", "list", "--json"]), a),
    },
    Tool {
        name: "worktree_info",
        description: "Details for one worktree: path, branch, agent status, git state, last commit, PR and Linear ticket.",
        params: &[TARGET, REPO],
        hints: READ_NET,
        timeout: SHORT,
        build: |a| repo(pos(cmd(&["worktree", "info", "--json"]), a, "target"), a),
    },
    Tool {
        name: "worktree_create",
        description: "Create a git worktree for a branch at <repo>/.claude/worktrees/<branch>. mode: fresh (new branch from base; fails if the branch exists), reuse (existing local branch), track (pull origin/<branch>), no_track (new local branch from base, ignoring the remote one).",
        params: &[
            req("branch", Kind::Str, "Branch name"),
            REPO,
            opt("name", Kind::Str, "Display name (nickname) for the worktree"),
            opt("base", Kind::Str, "Base branch for a new branch (default: the repo's main branch)"),
            opt("mode", Kind::Enum(MODES), "How to handle an existing branch (default: fresh)"),
            opt("tmp", Kind::Bool, "Check out an existing branch (someone else's, to look at) in the tmp space: tracks origin, or reuses a local-only branch; refused for a branch that doesn't exist; grouped apart on the dashboard and outside the worktree limit"),
        ],
        hints: WRITE,
        timeout: LONG,
        build: |a| {
            let mut argv = switch(with(with(repo(pos(cmd(&["worktree", "create", "--json"]), a, "branch"), a), a, "name", "--name"), a, "base", "--base"), a, "tmp", "--tmp");
            match a.str("mode").as_deref() {
                Some("reuse") => argv.push("--reuse".into()),
                Some("track") => argv.push("--track".into()),
                Some("no_track") => argv.push("--no-track".into()),
                _ => {}
            }
            argv
        },
    },
    Tool {
        name: "worktree_delete",
        description: "Delete a worktree (or, for the main checkout, just its non-default branch). Refuses uncommitted changes unless force is set.",
        params: &[
            TARGET,
            REPO,
            opt("force", Kind::Bool, "Delete even with uncommitted changes"),
            opt("delete_branch", Kind::Bool, "Also delete the local branch"),
            opt("delete_remote", Kind::Bool, "Also delete the branch on origin"),
        ],
        hints: DESTRUCTIVE,
        timeout: LONG,
        build: |a| {
            let argv = repo(pos(cmd(&["worktree", "delete"]), a, "target"), a);
            switch(switch(switch(argv, a, "force", "--force"), a, "delete_branch", "--delete-branch"), a, "delete_remote", "--delete-remote")
        },
    },
    Tool {
        name: "worktree_sync",
        description: "Sync am's database with `git worktree list` (picks up worktrees created or removed outside am).",
        params: &[REPO],
        hints: WRITE_SAME,
        timeout: SHORT,
        build: |a| repo(cmd(&["worktree", "sync"]), a),
    },
    Tool {
        name: "worktree_open",
        description: "Open a worktree in the user's configured IDE or terminal, resuming its last agent session if that setting is on.",
        params: &[TARGET, REPO],
        hints: LAUNCH,
        timeout: SHORT,
        build: |a| repo(pos(cmd(&["worktree", "open"]), a, "target"), a),
    },
    Tool {
        name: "repo_list",
        description: "List the repositories am tracks, with their worktree counts.",
        params: &[],
        hints: READ,
        timeout: SHORT,
        build: |_| cmd(&["repo", "list", "--json"]),
    },
    Tool {
        name: "repo_add",
        description: "Start tracking a git repository and import its worktrees.",
        params: &[req("path", Kind::Str, "Path to the repository")],
        hints: WRITE_SAME,
        timeout: SHORT,
        build: |a| pos(cmd(&["repo", "add", "--json"]), a, "path"),
    },
    Tool {
        name: "repo_remove",
        description: "Stop tracking a repository (nothing on disk is touched).",
        params: &[req("name_or_path", Kind::Str, "Repository name or path")],
        hints: DESTRUCTIVE,
        timeout: SHORT,
        build: |a| pos(cmd(&["repo", "remove"]), a, "name_or_path"),
    },
    Tool {
        name: "pr_show",
        description: "The GitHub pull request for a worktree's branch: number, state, CI checks, review decision, URL.",
        params: &[TARGET_OPT, REPO],
        hints: READ_NET,
        timeout: SHORT,
        build: |a| repo(pos(cmd(&["pr", "show", "--json"]), a, "target"), a),
    },
    Tool {
        name: "pr_open",
        description: "Open a worktree's pull request in the user's browser.",
        params: &[TARGET_OPT, REPO],
        hints: LAUNCH,
        timeout: SHORT,
        build: |a| repo(pos(cmd(&["pr", "open"]), a, "target"), a),
    },
    Tool {
        name: "linear_show",
        description: "The Linear ticket linked to a worktree's branch: identifier, title, state, project, URL.",
        params: &[TARGET_OPT, REPO],
        hints: READ_NET,
        timeout: SHORT,
        build: |a| repo(pos(cmd(&["linear", "show", "--json"]), a, "target"), a),
    },
    Tool {
        name: "linear_open",
        description: "Open a worktree's Linear ticket in the user's browser.",
        params: &[TARGET_OPT, REPO],
        hints: LAUNCH,
        timeout: SHORT,
        build: |a| repo(pos(cmd(&["linear", "open"]), a, "target"), a),
    },
    Tool {
        name: "agent_status",
        description: "Get the agent status am shows for a worktree, or set it with `set`.",
        params: &[
            opt("worktree", Kind::Str, "Worktree path (default: the agent's working directory)"),
            opt("set", Kind::Enum(STATUSES), "New status"),
        ],
        hints: WRITE_SAME,
        timeout: SHORT,
        build: |a| {
            let wt = a.str("worktree").unwrap_or_else(|| {
                std::env::current_dir().map(|d| d.display().to_string()).unwrap_or_else(|_| ".".into())
            });
            with(cmd(&["status", "--worktree", &wt]), a, "set", "--set")
        },
    },
    Tool {
        name: "settings_list",
        description: "All am settings with their current values.",
        params: &[],
        hints: READ,
        timeout: SHORT,
        build: |_| cmd(&["settings", "list", "--json"]),
    },
    Tool {
        name: "settings_get",
        description: "One am setting's value.",
        params: &[req("key", Kind::Str, "Setting key (camelCase, as in settings_list)")],
        hints: READ,
        timeout: SHORT,
        build: |a| pos(cmd(&["settings", "get", "--json"]), a, "key"),
    },
    Tool {
        name: "settings_set",
        description: "Change one am setting (validated like `am settings set`). worktreeSort and keyBindings can't be set this way.",
        params: &[req("key", Kind::Str, "Setting key"), req("value", Kind::Scalar, "New value")],
        hints: WRITE_SAME,
        timeout: SHORT,
        build: |a| pos(pos(cmd(&["settings", "set"]), a, "key"), a, "value"),
    },
    Tool {
        name: "settings_reset",
        description: "Reset every am setting to its default.",
        params: &[],
        hints: DESTRUCTIVE,
        timeout: SHORT,
        build: |_| cmd(&["settings", "reset"]),
    },
    Tool {
        name: "theme_list",
        description: "Built-in and custom colour themes.",
        params: &[],
        hints: READ,
        timeout: SHORT,
        build: |_| cmd(&["theme", "list", "--json"]),
    },
    Tool {
        name: "theme_show",
        description: "A theme's colours.",
        params: &[req("name", Kind::Str, "Theme name")],
        hints: READ,
        timeout: SHORT,
        build: |a| pos(cmd(&["theme", "show", "--json"]), a, "name"),
    },
    Tool {
        name: "theme_use",
        description: "Make a theme the active one.",
        params: &[req("name", Kind::Str, "Theme name")],
        hints: WRITE_SAME,
        timeout: SHORT,
        build: |a| pos(cmd(&["theme", "use"]), a, "name"),
    },
    Tool {
        name: "hooks_status",
        description: "Which agent CLIs have am's status hooks installed.",
        params: &[],
        hints: READ,
        timeout: SHORT,
        build: |_| cmd(&["hooks", "status", "--json"]),
    },
    Tool {
        name: "hooks_install",
        description: "Install am's status hooks into agent CLI settings files.",
        params: &[AGENTS],
        hints: WRITE_SAME,
        timeout: SHORT,
        build: |a| agents(cmd(&["hooks", "install"]), a),
    },
    Tool {
        name: "hooks_uninstall",
        description: "Remove am's status hooks from agent CLI settings files.",
        params: &[AGENTS],
        hints: DESTRUCTIVE,
        timeout: SHORT,
        build: |a| agents(cmd(&["hooks", "uninstall"]), a),
    },
    Tool {
        name: "script_show",
        description: "A repository's startup script (run after creating a worktree).",
        params: &[REPO],
        hints: READ,
        timeout: SHORT,
        build: |a| repo(cmd(&["script", "show", "--json"]), a),
    },
    Tool {
        name: "script_remove",
        description: "Delete a repository's startup script.",
        params: &[REPO],
        hints: DESTRUCTIVE,
        timeout: SHORT,
        build: |a| repo(cmd(&["script", "remove"]), a),
    },
    Tool {
        name: "doctor",
        description: "Health checks: app dir, database, settings, hooks, gh CLI, Linear key, themes.",
        params: &[],
        hints: READ,
        timeout: SHORT,
        build: |_| cmd(&["doctor", "--json"]),
    },
    Tool {
        name: "logs",
        description: "Recent lines of am's debug log.",
        params: &[
            opt("lines", Kind::Int, "Number of lines (default 50)"),
            opt("level", Kind::Enum(LEVELS), "Only this level"),
            opt("module", Kind::Str, "Only this module (e.g. hook, integrations, mcp)"),
        ],
        hints: READ,
        timeout: SHORT,
        build: |a| with(with(with(cmd(&["logs"]), a, "lines", "-n"), a, "level", "--level"), a, "module", "--module"),
    },
];

pub fn find(name: &str) -> Option<&'static Tool> {
    TOOLS.iter().find(|t| t.name == name)
}

fn kind_ok(kind: Kind, v: &Value) -> bool {
    match kind {
        Kind::Str => v.is_string(),
        Kind::Bool => v.is_boolean(),
        Kind::Int => v.as_u64().is_some(),
        Kind::Scalar => v.is_string() || v.is_number() || v.is_boolean(),
        Kind::Enum(options) => v.as_str().is_some_and(|s| options.contains(&s)),
        Kind::StrList => v.as_array().is_some_and(|a| a.iter().all(Value::is_string)),
    }
}

fn kind_name(kind: Kind) -> String {
    match kind {
        Kind::Str => "a string".into(),
        Kind::Bool => "a boolean".into(),
        Kind::Int => "a non-negative integer".into(),
        Kind::Scalar => "a string, number or boolean".into(),
        Kind::Enum(o) => format!("one of {}", o.join(", ")),
        Kind::StrList => "an array of strings".into(),
    }
}

impl Tool {
    /// The `am` argv for a call, or why the arguments are wrong.
    pub fn argv(&self, args: &Map<String, Value>) -> Result<Vec<String>, String> {
        for (k, v) in args {
            let Some(p) = self.params.iter().find(|p| p.name == k) else {
                return Err(format!("unknown argument \"{k}\""));
            };
            if !v.is_null() && !kind_ok(p.kind, v) {
                return Err(format!("\"{k}\" must be {}", kind_name(p.kind)));
            }
            // Values become argv words: one starting with `-` would be read as a flag.
            let words = v.as_array().map(Vec::as_slice).unwrap_or(std::slice::from_ref(v));
            if words.iter().filter_map(Value::as_str).any(|s| s.starts_with('-')) {
                return Err(format!("\"{k}\" can't start with '-'"));
            }
        }
        if let Some(p) = self.params.iter().find(|p| p.required && args.get(p.name).is_none_or(Value::is_null)) {
            return Err(format!("missing required argument \"{}\"", p.name));
        }
        Ok((self.build)(&Args(args)))
    }

    /// The `tools/list` entry.
    pub fn describe(&self) -> Value {
        let mut props = Map::new();
        for p in self.params {
            let mut schema = match p.kind {
                Kind::Str => json!({"type": "string"}),
                Kind::Bool => json!({"type": "boolean"}),
                Kind::Int => json!({"type": "integer", "minimum": 0}),
                Kind::Scalar => json!({"type": ["string", "number", "boolean"]}),
                Kind::Enum(o) => json!({"type": "string", "enum": o}),
                Kind::StrList => json!({"type": "array", "items": {"type": "string"}}),
            };
            schema["description"] = p.description.into();
            props.insert(p.name.into(), schema);
        }
        let required: Vec<&str> = self.params.iter().filter(|p| p.required).map(|p| p.name).collect();
        let h = self.hints;
        json!({
            "name": self.name,
            "description": self.description,
            "inputSchema": {"type": "object", "properties": props, "required": required, "additionalProperties": false},
            "annotations": {
                "readOnlyHint": h.read_only,
                "destructiveHint": h.destructive,
                "idempotentHint": h.idempotent,
                "openWorldHint": h.open_world,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(tool: &str, args: Value) -> Result<Vec<String>, String> {
        find(tool).unwrap().argv(args.as_object().unwrap())
    }

    #[test]
    fn names_are_unique_and_schemas_list_every_param() {
        let mut names: Vec<&str> = TOOLS.iter().map(|t| t.name).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), TOOLS.len());
        for t in TOOLS {
            let d = t.describe();
            assert_eq!(d["inputSchema"]["properties"].as_object().unwrap().len(), t.params.len(), "{}", t.name);
            assert!(t.name.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'), "{}", t.name);
        }
    }

    #[test]
    fn arguments_become_cli_flags() {
        assert_eq!(argv("worktree_list", json!({})).unwrap(), ["worktree", "list", "--json"]);
        assert_eq!(
            argv("worktree_create", json!({"branch": "feat/x", "repo": "/r", "base": "dev", "mode": "track"})).unwrap(),
            ["worktree", "create", "--json", "feat/x", "--repo", "/r", "--base", "dev", "--track"]
        );
        assert_eq!(
            argv("worktree_create", json!({"branch": "x", "tmp": true})).unwrap(),
            ["worktree", "create", "--json", "x", "--tmp"]
        );
        assert_eq!(
            argv("worktree_delete", json!({"target": "feat/x", "force": true, "delete_branch": false})).unwrap(),
            ["worktree", "delete", "feat/x", "--force"]
        );
        assert_eq!(argv("settings_set", json!({"key": "maxWorktrees", "value": 5})).unwrap(), ["settings", "set", "maxWorktrees", "5"]);
        assert_eq!(argv("settings_set", json!({"key": "ide", "value": "zed"})).unwrap(), ["settings", "set", "ide", "zed"]);
        assert_eq!(
            argv("hooks_install", json!({"agents": ["claude", "gemini"]})).unwrap(),
            ["hooks", "install", "--agent", "claude", "--agent", "gemini"]
        );
        assert_eq!(argv("logs", json!({"lines": 10, "level": "warn"})).unwrap(), ["logs", "-n", "10", "--level", "warn"]);
        assert_eq!(argv("pr_show", json!({"target": null})).unwrap(), ["pr", "show", "--json"]);
        assert_eq!(
            argv("agent_status", json!({"worktree": "/w", "set": "done"})).unwrap(),
            ["status", "--worktree", "/w", "--set", "done"]
        );
    }

    #[test]
    fn bad_arguments_are_refused() {
        assert_eq!(argv("worktree_info", json!({})).unwrap_err(), "missing required argument \"target\"");
        assert_eq!(argv("worktree_list", json!({"repoo": "/r"})).unwrap_err(), "unknown argument \"repoo\"");
        assert!(argv("worktree_create", json!({"branch": "x", "mode": "yolo"})).unwrap_err().contains("one of fresh"));
        assert!(argv("worktree_delete", json!({"target": "x", "force": "yes"})).is_err());
        assert!(argv("logs", json!({"lines": -1})).is_err());
        assert!(argv("agent_status", json!({"set": "busy"})).is_err());
        assert_eq!(argv("worktree_delete", json!({"target": "--force"})).unwrap_err(), "\"target\" can't start with '-'");
        assert!(argv("hooks_uninstall", json!({"agents": ["claude", "-x"]})).is_err());
    }
}
