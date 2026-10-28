//! Nous Research's Hermes Agent (`hermes`) as a built-in plugin: the
//! manifest `hermes.json`, a YAML hook block and the shell-hook allowlist.
//!
//! Hooks are "shell hooks" in `$HERMES_HOME/config.yaml` (`~/.hermes`):
//! `hooks: {<event>: [{command, timeout}]}`. Hermes splits `command` with
//! `shlex` and runs it without a shell, so the native command is written as
//! is and the `cat | am hook-event` fallback goes through `sh -c`. The
//! payload on stdin is `{hook_event_name, tool_name, tool_input, session_id,
//! cwd, profile, extra}`; there's no project-dir variable, so am-hook uses
//! the payload's `cwd` (Hermes's working directory).
//!
//! Quirks:
//! - YAML can't repeat a key (Hermes parses with ruamel, which rejects
//!   duplicates), so am's block can't simply be appended when the user has a
//!   top-level `hooks:` of their own: it then goes inside that mapping
//!   ([`place`]). An event the user's `hooks:` already has (a `pre_tool_call`
//!   guard, a `post_tool_call` formatter) is left out of am's block with a
//!   WARN; only `pre_llm_call` and `on_session_end`, without which no turn
//!   shows, are refused, as is an inline `hooks: {}`.
//! - Hermes only runs a shell hook whose exact `(event, command)` pair is in
//!   `shell-hooks-allowlist.json` beside the config (a foreground `hermes
//!   chat` asks once per pair; the gateway, TUI and cron just skip it).
//!   Installing writes those approvals for am's own commands and drops stale
//!   ones; uninstalling removes them ([`update_allowlist`]). Nothing else in
//!   the file is touched. The gateway and cron then run am's hooks too, with
//!   the gateway's own directory as `cwd`: [`normalize`] drops every event
//!   whose `extra.platform` isn't `cli`/`tui` (Telegram, Discord, cron…).
//! - `on_session_end` fires after every turn (completed, failed or
//!   interrupted), so it is the turn's `Stop`; `post_llm_call` (successful
//!   turns only) isn't installed, since a second `Stop` would turn `done`
//!   into `idle`. `on_session_finalize` is the real session end.
//! - After the turn, Hermes may start a background memory/skill review: a
//!   forked agent sharing the session id whose `pre_llm_call` and
//!   `on_session_end` are skipped but whose tool calls still fire
//!   `pre_tool_call`/`post_tool_call` (and carry no `platform`). Every turn
//!   opens with `pre_llm_call`, so tool and human-input events are
//!   [`turn_bound`](crate::status::HookEvent::turn_bound): outside a turn
//!   (`done`, `idle`, no status yet) they change nothing.
//! - Waiting comes from `on_human_input_request`/`_resolved` (Hermes from
//!   2026-10-05 on): they carry the session id and fire only when a person
//!   is needed (approval, sudo password, clarify), so automatic ("smart")
//!   approvals don't flash `waiting`. Older Hermes logs them as unknown
//!   events and shows no approval waits (a `clarify` question still waits).
//! - Profiles (`~/.hermes/profiles/<name>/`) each have their own config;
//!   only `$HERMES_HOME` (default `~/.hermes`) gets am's hooks, and it is
//!   what detection looks for.
//!
//! Resume: `hermes --resume <id>` / `--continue`. Session discovery uses the
//! sessions am's hooks recorded.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::embedded::Embedded;
use super::manifest::Manifest;
use crate::status::HookEvent;
use crate::hooks_installer::{HookContext, HookInstall, TextBlock, append_block, is_am_hook_command, read_settings, write_settings, yaml_string};

pub static HERMES: Embedded = Embedded::new(|| Manifest::builtin(include_str!("hermes.json")), None)
    .with_presence(|| hermes_home().is_dir())
    .with_normalize(normalize)
    .with_hook_file(|| (hermes_home().join("config.yaml"), "hooks"))
    .with_hook_install(|| HookInstall::TextBlock(TextBlock { comment: "#", render }))
    .with_block_placement(|text, block, _| place(text, block).map(Some))
    .with_hook_companion(allowlist);

/// File beside `config.yaml` holding the approved `(event, command)` pairs.
pub const ALLOWLIST: &str = "shell-hooks-allowlist.json";

/// `$HERMES_HOME`, else `~/.hermes`.
pub fn hermes_home() -> PathBuf {
    match std::env::var_os("HERMES_HOME").filter(|v| !v.is_empty()) {
        Some(home) => PathBuf::from(home),
        None => super::home_join(".hermes"),
    }
}

/// The surfaces whose turns are shown: the terminal ones, whose `cwd` is
/// where Hermes works. A payload without a `platform` (tool events) passes.
const SHOWN_PLATFORMS: [&str; 3] = ["", "cli", "tui"];

/// The manifest's mapping, minus gateway/cron turns, with the events that
/// only make sense inside a turn marked `turn_bound` (see the module docs).
pub fn normalize(m: &Manifest, payload: &Value, event: Option<&str>) -> HookEvent {
    let platform = payload.pointer("/extra/platform").and_then(Value::as_str).unwrap_or("");
    if !SHOWN_PLATFORMS.contains(&platform) {
        return HookEvent { event: "ignored".into(), ..HookEvent::default() };
    }
    let mut ev = m.normalize(payload, event);
    ev.turn_bound = matches!(ev.event.as_str(), "PreToolUse" | "PostToolUse" | "PermissionRequest");
    ev
}

/// Events am's block can't do without: the turn's start and end.
const REQUIRED: [&str; 2] = ["pre_llm_call", "on_session_end"];

/// `(event, command)` per hook, exactly as written to the config and the
/// allowlist (Hermes matches the command string).
fn commands(ctx: &HookContext) -> Vec<(String, String)> {
    ctx.hooks
        .iter()
        .map(|h| {
            let cmd = if ctx.native { h.command.clone() } else { format!("sh -c '{}'", h.command.replace('\'', r"'\''")) };
            (h.event.clone(), cmd)
        })
        .collect()
}

/// The block's body as a top-level `hooks:` mapping.
fn render(ctx: &HookContext) -> String {
    let secs = ctx.hooks.first().map(|h| h.timeout_secs()).unwrap_or(5);
    let mut out = String::from("hooks:\n");
    for (event, cmd) in commands(ctx) {
        out.push_str(&format!("  {event}:\n    - command: {}\n      timeout: {secs}\n", yaml_string(&cmd)));
    }
    out
}

fn other(msg: String) -> std::io::Error {
    std::io::Error::other(msg)
}

/// The value after a top-level `hooks:` key on its line (comment dropped), or
/// `None` when the line isn't that key.
fn top_level_hooks(line: &str) -> Option<&str> {
    let rest = ["hooks", "\"hooks\"", "'hooks'"].iter().find_map(|k| line.strip_prefix(k))?;
    let rest = rest.trim_start_matches([' ', '\t']).strip_prefix(':')?;
    if !(rest.is_empty() || rest.starts_with([' ', '\t', '\n', '\r'])) {
        return None;
    }
    let value = rest.trim();
    if value.starts_with('#') {
        return Some("");
    }
    Some(value.split(" #").next().unwrap_or("").trim_end())
}

/// A line with YAML content (not blank, not a comment).
fn significant(line: &str) -> bool {
    let t = line.trim();
    !t.is_empty() && !t.starts_with('#')
}

fn indent_of(line: &str) -> &str {
    &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
}

/// A mapping key at the start of a line (`on_session_end:` → `on_session_end`).
fn key_of(line: &str) -> Option<&str> {
    let t = line.trim_start();
    let (key, _) = t.split_once(':')?;
    Some(key.trim().trim_matches(['"', '\'']))
}

/// Block placement (`Harness::place_hook_block`) for `config.yaml`: appended when the file has no
/// top-level `hooks:` key, else nested under it (re-indented to its
/// children, right below the key) so the key isn't repeated, leaving out
/// the events the user's `hooks:` already has. Refuses an inline value
/// (`hooks: {}`) and a clash on a [`REQUIRED`] event.
pub fn place(text: &str, block: &str) -> std::io::Result<String> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let Some(at) = lines.iter().position(|l| top_level_hooks(l).is_some()) else {
        return Ok(append_block(text, block));
    };
    let value = top_level_hooks(lines[at]).unwrap_or("");
    if !value.is_empty() {
        return Err(other(format!(
            "config.yaml sets `hooks: {value}` on one line; am can only add its hooks under a block `hooks:` key. Change that line to `hooks:` (entries, if any, on the lines below it), then retry"
        )));
    }
    let children: Vec<&str> = lines[at + 1..].iter().copied().take_while(|l| !significant(l) || l.starts_with([' ', '\t'])).collect();
    let first = children.iter().find(|l| significant(l));
    let indent = first.map(|l| indent_of(l)).unwrap_or("  ");
    if first.is_some_and(|l| l.trim_start().starts_with('-')) {
        return Err(other("`hooks:` in config.yaml is a list, not a map of events; fix it, then retry".into()));
    }
    let taken: Vec<&str> = children.iter().filter(|l| significant(l) && indent_of(l) == indent).filter_map(|l| key_of(l)).collect();

    let body: Vec<&str> = block.lines().collect();
    let [begin, inner @ .., end] = body.as_slice() else {
        return Err(other("empty hook block".into()));
    };
    if inner.first() != Some(&"hooks:") {
        return Err(other("am's Hermes block must start with `hooks:`".into()));
    }
    let ours: Vec<&str> = inner.iter().filter(|l| l.starts_with("  ") && !l.starts_with("   ")).filter_map(|l| key_of(l)).collect();
    let clash: Vec<&str> = ours.iter().copied().filter(|e| taken.contains(e)).collect();
    let list = |events: &[&str]| events.iter().map(|e| format!("`{e}`")).collect::<Vec<_>>().join(", ");
    let required: Vec<&str> = clash.iter().copied().filter(|e| REQUIRED.contains(e)).collect();
    if !required.is_empty() {
        return Err(other(format!(
            "config.yaml's `hooks:` already has {}, which am needs for its own hook too, and YAML can't repeat a key. Move your entries for it into a script that also runs am's hook command, or remove them, then retry",
            list(&required)
        )));
    }
    if !clash.is_empty() {
        crate::warn!("hooks", "Hermes: config.yaml's `hooks:` already has {}; am's hooks for them are left out", list(&clash));
    }
    // am's entries, less the events the user's `hooks:` has: each event's
    // key line and the indented lines under it.
    let mut keep = true;
    let inner: Vec<&str> = inner
        .iter()
        .copied()
        .filter(|l| {
            if l.starts_with("  ") && !l.starts_with("   ") {
                keep = !key_of(l).is_some_and(|k| clash.contains(&k));
            }
            keep || !l.starts_with("  ")
        })
        .collect();

    let mut nested = format!("{indent}{begin}\n");
    for l in &inner[1..] {
        nested.push_str(&format!("{indent}{}\n", l.strip_prefix("  ").unwrap_or(l)));
    }
    nested.push_str(&format!("{indent}{end}\n"));
    let head = lines[..=at].concat();
    let nl = if head.ends_with('\n') { "" } else { "\n" };
    Ok(format!("{head}{nl}{nested}{}", lines[at + 1..].concat()))
}

/// Hook companion (`Harness::hook_companion`): approve am's commands in Hermes's allowlist on
/// install, drop them on uninstall.
fn allowlist(ctx: Option<&HookContext>) -> std::io::Result<()> {
    let now = time::OffsetDateTime::now_utc()
        .format(time::macros::format_description!("[year]-[month]-[day]T[hour]:[minute]:[second]Z"))
        .unwrap_or_default();
    update_allowlist(&hermes_home().join(ALLOWLIST), ctx.map(commands).as_deref(), &now)
}

/// Set am's approvals in the allowlist at `path` to exactly `hooks` (none on
/// uninstall): an am command not among them (an old binary path) is
/// dropped, a missing pair added with `approved_at: now`, an existing one
/// kept as it is, and every other entry and key left alone. Holds Hermes's
/// own lock (`flock` on `<path>.lock`). Unchanged → not written; emptied by
/// the uninstall → the file is removed.
pub fn update_allowlist(path: &Path, hooks: Option<&[(String, String)]>, now: &str) -> std::io::Result<()> {
    if hooks.is_none() && !path.exists() {
        return Ok(());
    }
    let _lock = lock(path)?;
    let mut root = read_settings(path)?;
    let approvals = match root.get("approvals") {
        None => Vec::new(),
        Some(Value::Array(a)) => a.clone(),
        Some(_) => return Err(other(format!("`approvals` in {} is not a list; not modifying it", path.display()))),
    };
    let wanted = hooks.unwrap_or_default();
    let pair = |a: &Value| (a.get("event").and_then(Value::as_str).map(str::to_owned), a.get("command").and_then(Value::as_str).map(str::to_owned));
    let mut kept: Vec<Value> = approvals
        .iter()
        .filter(|a| match pair(a) {
            (Some(e), Some(c)) if is_am_hook_command(&c) => wanted.contains(&(e, c)),
            _ => true,
        })
        .cloned()
        .collect();
    for (event, command) in wanted {
        if !kept.iter().any(|a| pair(a) == (Some(event.clone()), Some(command.clone()))) {
            kept.push(json!({ "event": event, "command": command, "approved_at": now }));
        }
    }
    if kept == approvals && (hooks.is_none() || root.contains_key("approvals")) {
        return Ok(());
    }
    if hooks.is_none() && kept.is_empty() && root.len() == 1 {
        std::fs::remove_file(path)?;
        crate::info!("hooks", "Removed am's Hermes shell-hook approvals: deleted {}", path.display());
        return Ok(());
    }
    let changed = kept.len().abs_diff(approvals.len());
    root.insert("approvals".into(), Value::Array(kept));
    write_settings(path, &root)?;
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    crate::info!("hooks", "Updated am's Hermes shell-hook approvals in {} ({changed} added/removed)", path.display());
    Ok(())
}

/// Hermes's allowlist lock: an exclusive `flock` on `<path>.lock`, released
/// when the returned file is dropped.
fn lock(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::fd::AsRawFd;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".lock");
    let file = std::fs::OpenOptions::new().create(true).append(true).open(path.with_file_name(name))?;
    loop {
        // SAFETY: flock on a descriptor this function owns.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } == 0 {
            return Ok(file);
        }
        let e = std::io::Error::last_os_error();
        if e.kind() != std::io::ErrorKind::Interrupted {
            return Err(e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::Harness;
    use crate::status::{Status::*, map_event_to_status};

    fn ctx(native: bool) -> HookContext {
        HookContext::new(&HERMES, native.then_some(Path::new("/opt/am/am-hook")))
    }

    #[test]
    fn a_hermes_turn_drives_the_status_rules() {
        let ev = |p: Value| HERMES.normalize(&p, None);
        let step = |cur, p: Value| map_event_to_status(&ev(p), cur, 0);
        let base = |event: &str, extra: Value| {
            let mut p = json!({"hook_event_name": event, "tool_name": null, "tool_input": null, "session_id": "20261028_1", "cwd": "/w", "profile": "default", "extra": {}});
            p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            p
        };
        let s = step(None, base("on_session_start", json!({"extra": {"model": "m", "platform": "cli"}})));
        assert_eq!(s, Some(Idle));
        let s = step(s, base("pre_llm_call", json!({"extra": {"user_message": "fix it", "is_first_turn": true}})));
        assert_eq!(s, Some(Executing));
        let tool = base("pre_tool_call", json!({"tool_name": "terminal", "tool_input": {"command": "cargo test"}}));
        assert_eq!(ev(tool.clone()).tool_name.as_deref(), Some("Bash"));
        assert_eq!(ev(tool.clone()).cwd.as_deref(), Some("/w"));
        let s = step(s, tool);
        assert_eq!(s, Some(Executing));
        // A person is needed (approval, sudo, clarify): the session's own event.
        let ask = base("on_human_input_request", json!({"extra": {"kind": "sudo", "request_id": "r1", "platform": "cli"}}));
        assert_eq!(ev(ask.clone()).session_id.as_deref(), Some("20261028_1"));
        let s = step(s, ask);
        assert_eq!(s, Some(Waiting));
        let s = step(s, base("on_human_input_resolved", json!({"extra": {"kind": "sudo", "outcome": "answered"}})));
        assert_eq!(s, Some(Executing));
        let s = step(s, base("post_tool_call", json!({"tool_name": "terminal"})));
        assert_eq!(s, Some(Executing));
        let s = step(s, base("on_session_end", json!({"extra": {"completed": true, "interrupted": false}})));
        assert_eq!(s, Some(Done));
        // The next turn starts from done; a clarify question waits.
        let s2 = step(s, base("pre_llm_call", json!({})));
        assert_eq!(s2, Some(Executing));
        assert_eq!(step(s2, base("pre_tool_call", json!({"tool_name": "clarify"}))), Some(Waiting));
        assert_eq!(step(s, base("on_session_finalize", json!({}))), Some(Idle));
        // Only one turn-end event is installed, so `done` isn't turned into `idle`.
        let stops: Vec<String> = HERMES.hook_events().into_iter().filter(|e| ev(json!({"hook_event_name": e})).event == "Stop").collect();
        assert_eq!(stops, ["on_session_end"]);
        assert!(!HERMES.hook_events().contains(&"post_llm_call".to_string()));
        // After the turn, the background review fork's tool calls (same
        // session id, no pre_llm_call) leave it done; so does a human-input
        // request the fork auto-denies.
        assert_eq!(step(s, base("pre_tool_call", json!({"tool_name": "memory", "extra": {"turn_id": "fork"}}))), None);
        assert_eq!(step(s, base("post_tool_call", json!({"tool_name": "read_file"}))), None);
        assert_eq!(step(s, base("on_human_input_request", json!({}))), None);
        assert_eq!(step(None, base("pre_tool_call", json!({"tool_name": "terminal"}))), None);
    }

    #[test]
    fn gateway_and_cron_turns_are_ignored() {
        let ev = |event: &str, platform: &str| HERMES.normalize(&json!({"hook_event_name": event, "session_id": "s", "cwd": "/srv", "extra": {"platform": platform}}), None).event;
        for p in ["telegram", "discord", "cron", "api_server"] {
            assert_eq!(ev("pre_llm_call", p), "ignored", "{p}");
            assert_eq!(ev("on_human_input_request", p), "ignored", "{p}");
        }
        for p in ["cli", "tui", ""] {
            assert_eq!(ev("pre_llm_call", p), "UserPromptSubmit", "{p}");
        }
        // Tool events carry no platform; they're turn-bound instead.
        let tool = HERMES.normalize(&json!({"hook_event_name": "pre_tool_call", "session_id": "s", "cwd": "/srv", "extra": {}}), None);
        assert!(tool.turn_bound);
        assert!(!HERMES.normalize(&json!({"hook_event_name": "pre_llm_call", "session_id": "s"}), None).turn_bound);
    }

    #[test]
    fn detection_follows_hermes_home() {
        // `with_presence` checks `$HERMES_HOME` (the hook file's directory).
        assert_eq!(HERMES.is_present(), hermes_home().is_dir() || super::super::home_join(".hermes").is_dir());
        assert_eq!(HERMES.settings_path().parent(), Some(hermes_home().as_path()));
    }

    #[test]
    fn render_writes_one_entry_per_event() {
        let body = render(&ctx(true));
        let cmd = |e: &str| format!("\"/opt/am/am-hook\" hook-event --agent hermes --worktree \"\" --event {e}");
        assert!(body.starts_with(&format!(
            "hooks:\n  on_session_start:\n    - command: {}\n      timeout: 5\n  on_session_reset:\n    - command: {}\n      timeout: 5\n",
            yaml_string(&cmd("on_session_start")),
            yaml_string(&cmd("on_session_reset"))
        )), "{body}");
        assert_eq!(body.lines().count(), 1 + 3 * HERMES.hook_events().len());
        assert_eq!(
            body.lines().nth(2),
            Some(r#"    - command: "\"/opt/am/am-hook\" hook-event --agent hermes --worktree \"\" --event on_session_start""#)
        );
        // Hermes runs commands without a shell: the CLI fallback needs one.
        let fallback = render(&ctx(false));
        assert!(fallback.contains(r#"- command: "sh -c 'cat | am hook-event --agent hermes --worktree \"\" --event pre_llm_call'""#), "{fallback}");
        assert!(commands(&ctx(false)).iter().all(|(_, c)| is_am_hook_command(c)));
        assert!(commands(&ctx(true)).iter().all(|(_, c)| is_am_hook_command(c)));
        // Dangling-binary detection reads the program back out of the YAML.
        assert!(crate::hooks_installer::hook_programs_in_text(&body).iter().all(|p| p == Path::new("/opt/am/am-hook")));
    }

    fn block() -> String {
        crate::hooks_installer::block_text("#", "hermes", &render(&ctx(true)))
    }

    #[test]
    fn the_block_is_appended_without_a_hooks_key_and_nested_under_one() {
        let b = block();
        assert_eq!(place("", &b).unwrap(), b);
        let user = "model:\n  default: x\n";
        assert_eq!(place(user, &b).unwrap(), format!("{user}\n{b}"));
        // A commented-out example isn't a key.
        let commented = "# hooks:\n#   pre_tool_call: []\nmodel: x\n";
        assert_eq!(place(commented, &b).unwrap(), format!("{commented}\n{b}"));

        // Under the user's own `hooks:`, at its children's indent, right below the key.
        let mine = "model: x\nhooks:   # mine\n    subagent_stop:\n      - command: ~/.hermes/agent-hooks/log.sh\nhooks_auto_accept: false\n";
        let placed = place(mine, &b).unwrap();
        assert!(placed.starts_with("model: x\nhooks:   # mine\n    # agent-monitor hooks hermes begin\n    on_session_start:\n      - command: "), "{placed}");
        assert!(placed.ends_with("    # agent-monitor hooks hermes end\n    subagent_stop:\n      - command: ~/.hermes/agent-hooks/log.sh\nhooks_auto_accept: false\n"), "{placed}");
        assert!(!placed.contains("\nhooks:\n"), "no second hooks key");
        assert_eq!(crate::hooks_installer::remove_block(&placed, "#", "hermes").unwrap().as_deref(), Some(mine));
        // An empty block key at the end of the file.
        let empty = "model: x\nhooks:";
        let placed = place(empty, &b).unwrap();
        assert!(placed.starts_with("model: x\nhooks:\n  # agent-monitor hooks hermes begin\n  on_session_start:\n    - command: "), "{placed}");

        // An event the user already hooks is left out of am's block.
        let guard = "hooks:\n  pre_tool_call:\n    - command: ./guard.sh\n  post_tool_call: []\n";
        let placed = place(guard, &b).unwrap();
        assert_eq!(placed.matches("pre_tool_call:").count(), 1, "{placed}");
        assert_eq!(placed.matches("post_tool_call:").count(), 1, "{placed}");
        assert!(placed.contains("  pre_llm_call:\n    - command: ") && placed.contains("  on_session_end:\n"), "{placed}");
        assert!(placed.ends_with("  # agent-monitor hooks hermes end\n  pre_tool_call:\n    - command: ./guard.sh\n  post_tool_call: []\n"), "{placed}");
        assert_eq!(crate::hooks_installer::remove_block(&placed, "#", "hermes").unwrap().as_deref(), Some(guard));

        // Refused: an inline value, a list, and the turn's start or end.
        for (text, says) in [
            ("hooks: {}\n", "`hooks: {}` on one line"),
            ("hooks: null\n", "`hooks: null`"),
            ("hooks:\n  - command: x\n", "is a list"),
            ("hooks:\n  pre_tool_call:\n    - command: ./guard.sh\n  on_session_end: []\n", "already has `on_session_end`, which am needs"),
            ("hooks:\n  pre_llm_call: []\n", "already has `pre_llm_call`"),
        ] {
            let e = place(text, &b).unwrap_err().to_string();
            assert!(e.contains(says), "{text:?}: {e}");
        }
    }

    #[test]
    fn approvals_are_added_kept_replaced_and_removed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hermes/shell-hooks-allowlist.json");
        let hooks = commands(&ctx(true));
        update_allowlist(&path, None, "t0").unwrap();
        assert!(!path.exists(), "uninstall with no file creates nothing");

        // Fresh: one approval per hook, file private.
        update_allowlist(&path, Some(&hooks), "t1").unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let approvals = v["approvals"].as_array().unwrap();
        assert_eq!(approvals.len(), hooks.len());
        assert_eq!(approvals[0], json!({"event": "on_session_start", "command": hooks[0].1, "approved_at": "t1"}));
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);

        // The user's approvals and keys stay; a reinstall changes nothing.
        let mut v = v;
        v["approvals"].as_array_mut().unwrap().insert(0, json!({"event": "post_tool_call", "command": "~/.hermes/agent-hooks/fmt.sh", "approved_at": "t0"}));
        v["note"] = json!("mine");
        std::fs::write(&path, serde_json::to_string_pretty(&v).unwrap()).unwrap();
        let before = std::fs::read_to_string(&path).unwrap();
        update_allowlist(&path, Some(&hooks), "t2").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);

        // A new binary path replaces the old approvals.
        let moved = commands(&HookContext::new(&HERMES, Some(Path::new("/new/am-hook"))));
        update_allowlist(&path, Some(&moved), "t3").unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let cmds: Vec<&str> = v["approvals"].as_array().unwrap().iter().map(|a| a["command"].as_str().unwrap()).collect();
        assert_eq!(cmds.len(), 1 + hooks.len());
        assert!(cmds.iter().skip(1).all(|c| c.starts_with("\"/new/am-hook\"")), "{cmds:?}");

        // Uninstall keeps only what isn't am's.
        update_allowlist(&path, None, "t4").unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v, json!({"approvals": [{"event": "post_tool_call", "command": "~/.hermes/agent-hooks/fmt.sh", "approved_at": "t0"}], "note": "mine"}));

        // An allowlist am can't read stops it.
        std::fs::write(&path, "{\"approvals\": {}}").unwrap();
        assert!(update_allowlist(&path, Some(&hooks), "t5").is_err());
        std::fs::write(&path, "not json").unwrap();
        assert!(update_allowlist(&path, Some(&hooks), "t5").is_err());

        // Emptied by the uninstall: removed.
        std::fs::remove_file(&path).unwrap();
        update_allowlist(&path, Some(&hooks), "t6").unwrap();
        update_allowlist(&path, None, "t7").unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn resume_and_process() {
        assert_eq!(HERMES.resume_args(Some("20261028_abc"), false), ["--resume", "20261028_abc"]);
        assert_eq!(HERMES.resume_args(None, true), ["--continue"]);
        assert!(HERMES.matches_process("hermes", &[]));
        // A Python console script: `python3 …/bin/hermes chat`.
        assert!(HERMES.matches_process("Python", &["/usr/bin/python3".into(), "/Users/me/.local/bin/hermes".into(), "chat".into()]));
        assert!(!HERMES.matches_process("python3", &["/usr/bin/python3".into(), "server.py".into()]));
        assert!(matches!(HERMES.hook_install(), HookInstall::TextBlock(_)));
        assert!(HERMES.settings_path().ends_with("config.yaml"));
    }
}
