//! Atlassian Rovo Dev CLI (`acli rovodev run`) as a built-in plugin: the
//! manifest `rovodev.json`, hooks written as a YAML block, and session
//! discovery from Rovo Dev's session store.
//!
//! Quirks:
//! - Hooks live in `~/.rovodev/config.yml` under
//!   `eventHooks: { events: [{name, commands: [{command}]}] }`, a file that
//!   also holds the rest of Rovo Dev's settings. am owns a marked block
//!   ([`HookInstall::TextBlock`]). A config with no `eventHooks:` gets the
//!   whole block appended; one that has it gets am's events inserted inside
//!   the existing mapping ([`place`]), since a second top-level `eventHooks:`
//!   would be a duplicate YAML key. An inline value am can't extend without
//!   editing the user's lines (`eventHooks: {}`, `events: []`) refuses the
//!   install with a message saying what to change.
//! - Rovo Dev pipes a JSON payload to the hook's stdin, but it carries no
//!   session id and no cwd, so the command passes `--worktree "$PWD"` (the
//!   hook inherits Rovo Dev's environment) and am-hook updates the worktree's
//!   row directly. Tool events nest the tool under `attributes.tool_input`
//!   (`tool_name`, `args`). There's no timeout setting.
//! - Event names: `on_complete`, `on_error` and `on_tool_permission` are the
//!   documented ones; `on_session_start`/`_end`, `on_user_prompt` and
//!   `on_tool_start`/`_end` come from newer releases (Atlassian's own
//!   peon-ping adapter registers them). Without `on_user_prompt` a session
//!   never shows `executing`.
//! - Rovo Dev reads its config at startup: restart a running session after
//!   installing.
//! - Process: `acli rovodev …` (the `acli` binary also runs Jira and other
//!   commands, so the match is on a `rovodev` argument).
//!
//! Sessions: `~/.rovodev/sessions/<id>/metadata.json` (title, workspace…;
//! `sessions.persistenceDir` in config.yml moves the directory) next to
//! `session_context.json`. Resumed with `acli rovodev run --restore <id>`.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::Value;

use super::embedded::Embedded;
use super::manifest::{Manifest, expand_home};
use super::{HarnessKind, home_join, is_within};
use crate::hooks_installer::{HookContext, HookInstall, TextBlock, block_begin, block_end, yaml_string};
use crate::launch::Session;

pub static ROVODEV: Embedded = Embedded::new(|| Manifest::builtin(include_str!("rovodev.json")), Some(discover))
    .with_hook_install(|| HookInstall::TextBlock(TextBlock { comment: "#", render }))
    .with_block_placement(place);

const ID: &str = "rovodev";

/// The block appended to a config with no `eventHooks:` key.
fn render(ctx: &HookContext) -> String {
    format!("eventHooks:\n  events:\n{}", events(ctx, "    "))
}

/// One `- name:` list item per hook, at `indent`.
fn events(ctx: &HookContext, indent: &str) -> String {
    ctx.hooks
        .iter()
        .map(|h| format!("{indent}- name: {}\n{indent}  commands:\n{indent}    - command: {}\n", h.event, yaml_string(&h.command)))
        .collect()
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

/// A line that holds a key or item (not blank, not only a comment).
fn is_content(line: &str) -> bool {
    let t = line.trim();
    !t.is_empty() && !t.starts_with('#')
}

/// The value after `key:` on `line` (at `indent`), comment stripped; `None`
/// when the line isn't that key.
fn key_value<'a>(line: &'a str, indent: usize, key: &str) -> Option<&'a str> {
    let line = line.trim_end_matches(['\n', '\r']);
    let rest = line.strip_prefix(&" ".repeat(indent))?.strip_prefix(key)?.trim_start_matches(' ').strip_prefix(':')?;
    if !rest.is_empty() && !rest.starts_with([' ', '\t']) {
        return None;
    }
    let rest = rest.trim();
    Some(if rest.starts_with('#') { "" } else { rest })
}

fn refuse(what: &str) -> std::io::Error {
    std::io::Error::other(format!(
        "~/.rovodev/config.yml has {what}; am can only add its hooks to a block mapping. Rewrite it as `eventHooks:` / `  events:` on their own lines (or delete it), then install again"
    ))
}

/// Puts am's block inside an existing top-level `eventHooks:` mapping:
/// after its `events:` line (matching the indent of the items already
/// there), or as a new `events:` key right after `eventHooks:`. `None`
/// (no `eventHooks:`) lets the installer append [`render`]'s block.
fn place(text: &str, ctx: &HookContext) -> std::io::Result<Option<String>> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let Some(top) = lines.iter().position(|l| key_value(l, 0, "eventHooks").is_some()) else { return Ok(None) };
    if !key_value(lines[top], 0, "eventHooks").unwrap_or_default().is_empty() {
        return Err(refuse("an inline `eventHooks:` value"));
    }
    // The mapping's lines: up to the next top-level key.
    let end = (top + 1..lines.len()).find(|&i| is_content(lines[i]) && indent_of(lines[i]) == 0).unwrap_or(lines.len());
    let body = top + 1..end;
    let child = body.clone().find(|&i| is_content(lines[i])).map(|i| indent_of(lines[i])).unwrap_or(2);
    let (at, block) = match body.clone().find(|&i| key_value(lines[i], child, "events").is_some()) {
        Some(ev) => {
            if !key_value(lines[ev], child, "events").unwrap_or_default().is_empty() {
                return Err(refuse("an inline `eventHooks.events` value (such as `events: []`)"));
            }
            // Items may sit at the key's indent or deeper; match the first.
            let item = (ev + 1..end)
                .find(|&i| is_content(lines[i]))
                .filter(|&i| lines[i].trim_start().starts_with('-') && indent_of(lines[i]) >= child)
                .map(|i| indent_of(lines[i]))
                .unwrap_or(child + 2);
            let pad = " ".repeat(item);
            (ev + 1, format!("{pad}{}\n{}{pad}{}\n", block_begin("#", ID), events(ctx, &pad), block_end("#", ID)))
        }
        None => {
            let pad = " ".repeat(child);
            let items = events(ctx, &" ".repeat(child + 2));
            (top + 1, format!("{pad}{}\n{pad}events:\n{items}{pad}{}\n", block_begin("#", ID), block_end("#", ID)))
        }
    };
    let mut out: String = lines[..at].concat();
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&block);
    out.push_str(&lines[at..].concat());
    Ok(Some(out))
}

fn discover(root: &Path) -> Vec<Session> {
    discover_sessions_in(&sessions_dir(), root)
}

/// `sessions.persistenceDir` from config.yml, else `~/.rovodev/sessions`.
fn sessions_dir() -> PathBuf {
    let config = std::fs::read_to_string(home_join(".rovodev/config.yml")).unwrap_or_default();
    persistence_dir(&config).map(|d| expand_home(&d)).unwrap_or_else(|| home_join(".rovodev/sessions"))
}

/// The `persistenceDir` under the top-level `sessions:` mapping.
fn persistence_dir(config: &str) -> Option<String> {
    let lines: Vec<&str> = config.lines().collect();
    let top = lines.iter().position(|l| key_value(l, 0, "sessions") == Some(""))?;
    let children = lines[top + 1..].iter().take_while(|l| !is_content(l) || indent_of(l) > 0).filter(|l| is_content(l));
    let child = children.clone().next().map(|l| indent_of(l))?;
    let raw = children.filter_map(|l| key_value(l, child, "persistenceDir")).next()?;
    let v = match raw.chars().next() {
        Some(q @ ('"' | '\'')) => raw[1..].split(q).next().unwrap_or_default(),
        _ => raw.split(" #").next().unwrap_or_default().trim(),
    };
    (!v.is_empty()).then(|| v.to_owned())
}

/// Sessions whose workspace is `root` or below it, newest first (by the
/// later of `metadata.json` and `session_context.json`).
pub fn discover_sessions_in(sessions_dir: &Path, root: &Path) -> Vec<Session> {
    let root = root.to_string_lossy();
    let mut sessions = Vec::new();
    for dir in std::fs::read_dir(sessions_dir).into_iter().flatten().flatten() {
        let path = dir.path();
        let Some(id) = path.file_name().and_then(|n| n.to_str()).filter(|n| !n.starts_with('.')).map(str::to_owned) else { continue };
        let meta = path.join("metadata.json");
        let Some(cwd) = std::fs::read_to_string(&meta).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).and_then(|v| workspace(&v)) else {
            continue;
        };
        if !is_within(&cwd, &root) {
            continue;
        }
        let mtime = |p: &Path| p.metadata().and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
        let modified = mtime(&meta).max(mtime(&path.join("session_context.json")));
        sessions.push(Session { id, cwd, modified, agent: HarnessKind::ROVODEV });
    }
    // Ids are time-ordered, so they break ties newest first too.
    sessions.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| b.id.cmp(&a.id)));
    sessions
}

/// The session's workspace path (the key has varied between releases).
fn workspace(meta: &Value) -> Option<String> {
    ["workspace_path", "workspacePath", "workspace", "cwd", "working_directory", "workingDirectory"]
        .iter()
        .find_map(|k| meta.get(*k).and_then(Value::as_str))
        .map(|p| p.trim_end_matches('/'))
        .filter(|p| !p.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::Harness;
    use crate::hooks_installer::HookCommand;
    use crate::status::{Status::*, map_event_to_status};
    use serde_json::json;

    fn step(cur: Option<crate::status::Status>, event: &str, p: Value) -> Option<crate::status::Status> {
        map_event_to_status(&ROVODEV.normalize(&p, Some(event)), cur, 0)
    }

    #[test]
    fn a_rovodev_turn_drives_the_status_rules() {
        let s = step(None, "on_session_start", json!({}));
        assert_eq!(s, Some(Idle));
        let s = step(s, "on_user_prompt", json!({}));
        assert_eq!(s, Some(Executing));
        let perm = json!({"attributes": {"tool_input": {"tool_name": "bash", "args": {"command": "ls -la"}}}});
        let e = ROVODEV.normalize(&perm, Some("on_tool_permission"));
        assert_eq!((e.event.as_str(), e.tool_name.as_deref(), e.session_id.as_deref()), ("PermissionRequest", Some("Bash"), None));
        assert_eq!(e.tool_input, Some(json!({"command": "ls -la"})));
        let s = step(s, "on_tool_permission", perm.clone());
        assert_eq!(s, Some(Waiting));
        let s = step(s, "on_tool_start", perm);
        assert_eq!(s, Some(Executing));
        let s = step(s, "on_tool_end", json!({}));
        assert_eq!(s, Some(Executing));
        assert_eq!(step(s, "on_complete", json!({})), Some(Done));
        assert_eq!(step(s, "on_error", json!({})), Some(Idle));
        assert_eq!(step(Some(Done), "on_session_end", json!({})), Some(Idle));
    }

    fn ctx() -> HookContext {
        let hook = |event: &str| HookCommand {
            event: event.into(),
            command: format!("\"/bin/am-hook\" hook-event --agent rovodev --worktree \"$PWD\" --event {event}"),
            timeout_ms: 5000,
        };
        HookContext {
            agent_id: "rovodev".into(),
            program: "/bin/am-hook".into(),
            native: true,
            invocation: "\"/bin/am-hook\" hook-event".into(),
            hooks: vec![hook("on_complete"), hook("on_tool_permission")],
        }
    }

    const ITEMS: &str = r#"    - name: on_complete
      commands:
        - command: "\"/bin/am-hook\" hook-event --agent rovodev --worktree \"$PWD\" --event on_complete"
    - name: on_tool_permission
      commands:
        - command: "\"/bin/am-hook\" hook-event --agent rovodev --worktree \"$PWD\" --event on_tool_permission"
"#;

    #[test]
    fn renders_the_event_hooks_block() {
        assert_eq!(render(&ctx()), format!("eventHooks:\n  events:\n{ITEMS}"));
        // The real context: every event, with the PWD command.
        let real = HookContext::new(&ROVODEV, Some(Path::new("/bin/am-hook")));
        assert_eq!(real.hooks.len(), 8);
        assert_eq!(real.hooks[0].command, "\"/bin/am-hook\" hook-event --agent rovodev --worktree \"$PWD\" --event on_session_start");
    }

    #[test]
    fn no_event_hooks_key_appends() {
        assert!(place("", &ctx()).unwrap().is_none());
        assert!(place("sessions:\n  auto_restore: true\n# eventHooks: later\n", &ctx()).unwrap().is_none());
        // A nested key of that name isn't the top-level one.
        assert!(place("other:\n  eventHooks:\n    events:\n", &ctx()).unwrap().is_none());
    }

    #[test]
    fn inserts_into_an_existing_events_list() {
        let text = "agent:\n  streaming: true\neventHooks:\n  logFile: ~/.rovodev/event_hooks.log\n  events:\n  - name: on_complete\n    commands:\n    - command: afplay /System/Library/Sounds/Glass.aiff\nsessions:\n  auto_restore: false\n";
        let out = place(text, &ctx()).unwrap().unwrap();
        let items = ITEMS.lines().map(|l| format!("{}\n", &l[2..])).collect::<String>();
        let want = format!(
            "agent:\n  streaming: true\neventHooks:\n  logFile: ~/.rovodev/event_hooks.log\n  events:\n  # agent-monitor hooks rovodev begin\n{items}  # agent-monitor hooks rovodev end\n  - name: on_complete\n    commands:\n    - command: afplay /System/Library/Sounds/Glass.aiff\nsessions:\n  auto_restore: false\n"
        );
        assert_eq!(out, want);
        // Indented items keep their indent.
        let text = "eventHooks:\n    events:   # mine\n        - name: on_error\n          commands: []\n";
        let out = place(text, &ctx()).unwrap().unwrap();
        assert!(out.starts_with("eventHooks:\n    events:   # mine\n        # agent-monitor hooks rovodev begin\n        - name: on_complete\n"), "{out}");
        assert!(out.ends_with("        # agent-monitor hooks rovodev end\n        - name: on_error\n          commands: []\n"), "{out}");
    }

    #[test]
    fn adds_events_to_an_event_hooks_mapping_without_them() {
        let text = "eventHooks:\n  logFile: x.log\nother: 1";
        let out = place(text, &ctx()).unwrap().unwrap();
        assert_eq!(
            out,
            format!("eventHooks:\n  # agent-monitor hooks rovodev begin\n  events:\n{ITEMS}  # agent-monitor hooks rovodev end\n  logFile: x.log\nother: 1")
        );
        // An empty mapping at the end of a file without a final newline.
        let out = place("eventHooks:", &ctx()).unwrap().unwrap();
        assert!(out.starts_with("eventHooks:\n  # agent-monitor hooks rovodev begin\n  events:\n    - name: on_complete\n"), "{out}");
    }

    #[test]
    fn refuses_inline_values() {
        for text in ["eventHooks: {}\n", "eventHooks:\n  events: []\n", "eventHooks: {events: []}\n"] {
            let err = place(text, &ctx()).unwrap_err().to_string();
            assert!(err.contains("block mapping"), "{text}: {err}");
        }
    }

    #[test]
    fn reads_the_persistence_dir() {
        assert_eq!(persistence_dir("sessions:\n  auto_restore: true\n  persistenceDir: \"~/rovo/s\"\n"), Some("~/rovo/s".into()));
        assert_eq!(persistence_dir("sessions:\n  persistenceDir: /x # here\nother: 1\n"), Some("/x".into()));
        assert_eq!(persistence_dir("other:\n  persistenceDir: /x\n"), None);
        assert_eq!(persistence_dir("sessions:\n  auto_restore: true\nfoo:\n  persistenceDir: /x\n"), None);
    }

    #[test]
    fn discovers_sessions_under_the_root() {
        let dir = tempfile::tempdir().unwrap();
        let mk = |id: &str, meta: Value| {
            let d = dir.path().join(id);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("metadata.json"), meta.to_string()).unwrap();
            std::fs::write(d.join("session_context.json"), "{}").unwrap();
            std::thread::sleep(std::time::Duration::from_millis(15));
        };
        mk("a1", json!({"title": "old", "workspace_path": "/w/repo"}));
        mk("b2", json!({"title": "sub", "workspace_path": "/w/repo/pkg/"}));
        mk("c3", json!({"workspace_path": "/w/repo-other"}));
        mk("d4", json!({"title": "no workspace"}));
        std::fs::create_dir_all(dir.path().join("e5")).unwrap();
        let s = discover_sessions_in(dir.path(), Path::new("/w/repo"));
        let ids: Vec<&str> = s.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["b2", "a1"]);
        assert_eq!(s[0].cwd, "/w/repo/pkg");
        assert_eq!(s[0].agent, HarnessKind::ROVODEV);
        // A newer conversation in an older session moves it first.
        std::thread::sleep(std::time::Duration::from_millis(15));
        std::fs::write(dir.path().join("a1/session_context.json"), "{\"more\": 1}").unwrap();
        let ids: Vec<String> = discover_sessions_in(dir.path(), Path::new("/w/repo")).into_iter().map(|s| s.id).collect();
        assert_eq!(ids, ["a1", "b2"]);
        assert!(discover_sessions_in(&dir.path().join("missing"), Path::new("/w/repo")).is_empty());
    }

    #[test]
    fn resume_and_processes() {
        assert_eq!(ROVODEV.program(), "acli");
        assert_eq!(ROVODEV.resume_args(Some("abc"), true), ["rovodev", "run", "--restore", "abc"]);
        assert_eq!(ROVODEV.resume_args(None, true), ["rovodev", "run", "--restore"]);
        assert_eq!(ROVODEV.resume_args(None, false), ["rovodev", "run"]);
        assert!(ROVODEV.matches_process("acli", &["acli".into(), "rovodev".into(), "run".into()]));
        assert!(!ROVODEV.matches_process("acli", &["acli".into(), "jira".into(), "workitem".into()]));
        assert_eq!(ROVODEV.hook_command("on_complete", "am"), "am --agent rovodev --worktree \"$PWD\" --event on_complete");
    }
}
