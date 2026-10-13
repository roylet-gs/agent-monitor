# Agent providers: monitoring more than Claude Code

am started as a dashboard for Claude Code. This document describes how it was made agent-agnostic, why it is built this way, and how to add another agent CLI. Gemini CLI is the first agent added this way.

> **Harness, not model.** The pluggable unit is the *harness*: the CLI or IDE that runs the model, fires the hooks, writes the transcripts, owns the resume command and shows up in the process table. The model is an attribute of a session; Codex, Copilot, Cursor, OpenCode and others run many models, and Claude Code can sit behind a gateway. The code calls a plugin a `Harness` (`am_core::harness`, handle `HarnessKind`); persisted names keep the older word (`--agent` with alias `--harness`, the `agent` DB column, `defaultAgent`/`disabledAgents`, `~/.agent-monitor/agents/*.json`) so existing hooks, settings and 1.x keep working.

## What was Claude-specific

The core of am never depended on Claude itself. The status rules, the SQLite schema, worktrees, integrations and the TUI only need to know what an agent is doing. The Claude-specific parts were all at the edges:

| Concern | Where it lived | What was hard-coded |
|---|---|---|
| Hook install | `hooks_installer.rs` | `~/.claude/settings.json`, Claude's 11 event names, `$CLAUDE_PROJECT_DIR`, `--managed` |
| Hook payload | `status.rs` `HookEvent::from_json` | Claude's event names (`UserPromptSubmit`, `PreToolUse`, `Stop`…) and tool names (`AskUserQuestion`, `ExitPlanMode`, `Bash`…) |
| Session discovery | `launch.rs` | `~/.claude/projects/<encoded cwd>/<id>.jsonl` |
| Resume command | `launch.rs`, `am agent attach` | `claude --resume <id>`, `claude -c` |
| Process detection | `procscan.rs` | the `claude` executable (excluded from "running processes") |
| Subagents | `am-hook`, `subagents` table | `SubagentStart/Stop`, `agent_id`, `<session>/subagents/agent-<id>.meta.json` |
| Copy | wizard, help, doctor, key bar | "Claude hooks", `c claude` |

## Design

### Goal

New agent types must plug in without refactoring am. The DB, the status rules, the TUI, the CLI, settings, doctor and the process scan all go through an **open registry** of providers, and the aim is that nothing outside `crates/am-core/src/harness/` names a specific harness. Known exceptions still to move behind the plugin: the canonical tool names in `status.rs` (`AskUserQuestion`, `ExitPlanMode`…), `procscan::claude_shell_commands`, the `SubagentHandback` parked-subagent rule in `am-tui/src/data.rs`, and the places that special-case Claude as the default harness.

### Options considered

1. **Only external, executable plugins** (am spawns a plugin per hook event). This is fully open, but am's hook path runs on every tool call and has a ~3 ms budget, so spawning a process there costs latency and adds a failure mode.
2. **A closed set of compiled providers** (an enum with a `match` per agent). This is fast, but every new agent means edits across the codebase. This was the first cut, and it was rejected for that reason.
3. **An open registry of providers behind a trait, with two kinds of plugin** (chosen):
   - **Manifest plugins**: a JSON file in `~/.agent-monitor/agents/<id>.json`. It declares the hook file and events, native→canonical event, tool and field mappings, resume arguments, process patterns and an optional session-listing command. No code and no rebuild. Normalising is pure data lookups, so the hook path stays fast.
   - **Built-in plugins**: a Rust module implementing `Harness`, plus one entry in `harness::BUILTIN`. Use this when an agent needs logic a manifest can't express, such as Claude's transcript-`cwd` session discovery or Gemini's project-slug registry.

Gemini is built as **both**. Its mapping, hooks, resume arguments and process patterns are the manifest `crates/am-core/src/harness/gemini.json`, in exactly the format users write. Only session discovery is Rust. This proves the manifest format can describe a real agent.

### Managing plugins

- **Settings → Agent plugins…** (TUI sub-page, `crates/am-tui/src/plugins_ui.rs`) lists every registered plugin with its source (`built-in` or the manifest file), whether it is on, its hook status and the default marker. It also shows manifest files that failed to load.
  - **space** turns a plugin on or off. Off removes am's hooks from that CLI, and on installs them (when the CLI is set up on this machine).
  - **i** (re)installs the hooks.
  - **d** makes the plugin the default agent that `Enter` starts in a worktree with no session yet.
- The state lives in `settings.json`: `disabledAgents` (ids) and `defaultAgent`. A disabled plugin is left out of `am hooks install`, the setup wizard and session discovery.
- `am doctor` checks every plugin's hooks and lists manifest files it skipped. `am hooks install|uninstall --agent <id>` and `am settings set defaultAgent <id>` accept any registered id.

### Canonical events

Claude Code's hook vocabulary is am's **canonical event format**. Each provider translates its CLI's native payload into a canonical `HookEvent`, and everything downstream is unchanged. That includes `map_event_to_status`, the done/delegating protections, the DB writes and the TUI. Claude's provider is the identity mapping, so the existing behaviour and tests stay exactly as they were.

```
gemini ─ BeforeTool ─▶ am-hook --agent gemini ─▶ Gemini::normalize ─▶ HookEvent{PreToolUse, Bash} ─┐
claude ─ PreToolUse ─▶ am-hook                ─▶ Claude::normalize ─▶ HookEvent{PreToolUse, Bash} ─┴▶ map_event_to_status ─▶ SQLite
```

### The trait and registry (`crates/am-core/src/harness/mod.rs`)

```rust
pub trait Harness: Sync + Send {
    fn id(&self) -> &str;                   // "gemini": --agent flag, DB `agent` column, settings
    fn name(&self) -> &str;                 // "Gemini CLI"
    fn short_name(&self) -> &str;           // "Gemini"
    fn program(&self) -> &str;              // "gemini"
    fn origin(&self) -> Origin;             // BuiltIn | Plugin(path)
    fn notes(&self) -> Option<&str>;        // caveat shown next to hook status
    fn is_present(&self) -> bool;           // config dir exists → install hooks by default
    fn settings_path(&self) -> PathBuf;     // ~/.gemini/settings.json
    fn hook_events(&self) -> Vec<String>;
    fn hook_entry(&self, event: &str, invocation: &str, native: bool) -> Value;
    fn normalize(&self, payload: &Value, event_override: Option<&str>) -> HookEvent;
    fn discover_sessions(&self, root: &Path) -> Vec<Session>;
    fn resume_args(&self, session_id: Option<&str>, continue_latest: bool) -> Vec<String>;
    fn context_usage(&self, root: &Path, session_id: &str) -> Option<ContextUsage>; // default None
    fn usage_files(&self, since: SystemTime) -> Vec<PathBuf>;           // default empty
    fn usage_record(&self, line: &str) -> Option<UsageRecord>;          // default None
    fn matches_process(&self, name: &str, argv: &[String]) -> bool;
}
```

`usage_files` and `usage_record` feed today's token total (`am_core::usage`). The first lists transcript files modified since local midnight, subagent transcripts included. The second parses one line into `{ id, at, cwd, tokens }`, with `tokens` summing every billed class (input, cache creation, cache read, output). The tracker reads only appended bytes, de-duplicates by `id`, and attributes each `cwd` to the most specific worktree root. Claude implements both. Gemini and manifest plugins use the defaults for now, so their sessions don't count yet.

`HarnessKind` is a copyable handle to a registered provider, compared by id. `harness::all()` returns the built-ins followed by manifest plugins, which are loaded once per process. `HarnessKind::get(id)` checks the built-ins first, so a Claude or Gemini hook never reads the plugin directory.

The shared machinery around the registry is:

- **`hooks_installer`** has `install_hooks(kind)`, `uninstall_hooks(kind)` and `is_hooks_installed(kind)`. They share the existing "replace our entries, keep everyone else's" rules and the `is_am_hook_command` marker. A settings file that exists but can't be parsed is now an error rather than being treated as empty, because Gemini allows comments in `settings.json` and a rewrite would silently drop them.
- **`am-hook`** takes `--agent <id>` (default `claude`, so existing Claude hook entries keep working unchanged), normalises the event through the provider and records the agent on the status row. Older binaries ignore the unknown flag.
- **DB schema v3** adds a nullable `agent TEXT` column to `agent_status` and `standalone_sessions`. NULL means Claude, which is what every 1.x row is. An unknown id, such as a plugin that has since been removed, also reads as Claude. The change is additive, so 1.x can still read the DB.
- **`launch`**: `Session` and `Resume` carry an `HarnessKind`. `discover_sessions` merges every enabled provider's sessions. `resume_for(root, known_agent, default_agent)` uses the agent the hooks last saw, otherwise the newest session of any agent, otherwise `defaultAgent`. `agent_command` builds `<program> <resume args>`.
- **`procscan`** excludes any registered agent CLI, and anything it spawns directly, from a worktree's running processes.
- **TUI**: a non-Claude row gets a blue `✦ <id>` badge (`✦` below 100 columns). The wizard's hook step installs for every detected, enabled plugin.

### What stays Claude-only

Subagent tracking (the `subagents` table, `delegating`, parked subagents) needs per-subagent ids in the hook payload. Today only Claude Code provides them. A provider that doesn't simply never emits `SubagentStart`/`SubagentStop` or `agent_id`, and that is safe: the rules then never produce `delegating`.

## Gemini CLI

Verified against `google-gemini/gemini-cli` at `51617c1` (0.65.0-nightly, 2026-10-12): `docs/hooks/*`, `packages/core/src/hooks/*`, `config/storage.ts`, `config/projectRegistry.ts` and `services/chatRecordingService.ts`.

### Hooks

All of the following is declared in `crates/am-core/src/harness/gemini.json`.

Gemini's hook system is modelled on Claude Code's. It has the same `{"hooks": {"<Event>": [{"matcher", "hooks": [{"type": "command", "command", "timeout"}]}]}}` shape in `~/.gemini/settings.json`, the same common stdin keys (`session_id`, `transcript_path`, `cwd`, `hook_event_name`), and `timeout` in milliseconds. Hooks are on by default (`hooksConfig.enabled`).

The entry am writes for each event looks like this:

```json
{"matcher": "*", "hooks": [{"type": "command", "name": "agent-monitor", "timeout": 5000,
  "command": "\"/path/am-hook\" hook-event --agent gemini --worktree \"$GEMINI_PROJECT_DIR\" --event BeforeTool"}]}
```

| Gemini event | Canonical | Notes |
|---|---|---|
| `SessionStart` / `SessionEnd` | same | `source` / `reason` are ignored |
| `BeforeAgent` | `UserPromptSubmit` | the user submitted a prompt |
| `AfterAgent` | `Stop` | `prompt_response` → `last_assistant_message`; `stop_hook_active` is passed through |
| `BeforeTool` / `AfterTool` | `PreToolUse` / `PostToolUse` | tool names are mapped, see below |
| `Notification` (`ToolPermission`) | `Notification` (`permission_prompt`) | a confirmation prompt is on screen → `waiting` |
| `BeforeModel`, `AfterModel`, `BeforeToolSelection`, `PreCompress` | not installed | they carry no status information, and `AfterModel` fires per streamed chunk |

Built-in tool names are mapped to the Claude tool with the same role. This lets the existing rules apply: `ask_user` and `exit_plan_mode` mean waiting, and `enter_plan_mode` means planning. The full mapping is:

| Gemini tool | Canonical tool |
|---|---|
| `run_shell_command` | `Bash` |
| `read_file` | `Read` |
| `write_file` | `Write` |
| `replace` | `Edit` |
| `glob` | `Glob` |
| `grep_search` | `Grep` |
| `web_fetch` | `WebFetch` |
| `google_web_search` | `WebSearch` |
| `ask_user` | `AskUserQuestion` |
| `enter_plan_mode` | `EnterPlanMode` |
| `exit_plan_mode` | `ExitPlanMode` |
| `invoke_agent` | `Agent` |

MCP tools (`mcp_<server>_<tool>`) keep their names.

Gemini-specific constraints:

- **Silent hooks.** Gemini shows a hook's stdout, or its stderr when stdout is empty, to the user as a system message. `am-hook` never prints on the hook path; `crates/am-hook/tests/hook.rs` asserts this.
- **Folder trust.** With `folderTrust` enabled (the default), Gemini runs no settings hooks in an untrusted folder, user-level ones included. Trust the repository's parent directory so new worktrees inherit it. `am doctor` mentions this.
- **No subagent ids.** Subagents run as an `invoke_agent` tool call, and their tools fire `BeforeTool`/`AfterTool` with the parent's `session_id` and no agent id. The worktree shows `executing` while one runs, which is accurate, because the main turn is waiting on it.

### Sessions and resume

- `~/.gemini/projects.json` is `{"projects": {"<abs launch dir>": "<slug>"}}`. A project is the directory `gemini` was launched in, not the git root.
- Transcripts are `~/.gemini/tmp/<slug>/chats/session-<YYYY-MM-DDTHH-MM>-<id8>.jsonl`. The first line is metadata (`sessionId`, `projectHash`, `kind: "main" | "subagent"`). am lists every registered project inside the worktree, skips subagent transcripts, and sorts by mtime.
- Resuming uses `gemini --resume <uuid>`, or `gemini --resume latest`. Both are scoped to the cwd's project, so am `cd`s to the session's directory first when it is a subdirectory.

### Process detection

Gemini is a Node script that re-spawns itself (`node [flags] …/@google/gemini-cli/bundle/gemini.js`). Its manifest therefore matches on argv rather than the process name: `"process": {"names": ["gemini", "gemini.js"], "argvContains": ["/@google/gemini-cli/"]}`. Gemini and anything it spawns directly, such as MCP servers, are excluded from a worktree's "running processes".

### Not yet verified live

The local `gemini` here is 0.1.9, which predates hooks, so the mapping was tested against the source and with replayed payloads: unit tests, `am-hook` integration tests and the Docker E2E seed. These were not checked against a real session:

- whether `AfterAgent` ever fires for a subagent (the source suggests not), and
- the exact `ps` argv strings.

To verify against a real session, upgrade with `npm i -g @google/gemini-cli`, run `am hooks install`, then use `AM_HOOK_CAPTURE=1` to save real payloads to `~/.agent-monitor/hook-captures/gemini-*.json`.

## Adding another agent

### As a manifest (no code)

Drop `~/.agent-monitor/agents/<id>.json` (or `$AM_DATA_DIR/agents/`) and restart am:

```json
{
  "id": "mycli", "name": "My CLI", "shortName": "MyCLI", "program": "mycli",
  "configDir": "~/.mycli",
  "hooks": { "settingsPath": "~/.mycli/settings.json", "projectDirVar": "MYCLI_PROJECT_DIR",
             "matcher": "*", "timeoutMs": 5000, "name": "agent-monitor" },
  "events": { "PromptSubmitted": "UserPromptSubmit", "TurnDone": "Stop",
              "ToolStart": "PreToolUse", "ToolEnd": "PostToolUse", "Approval": "Notification" },
  "tools": { "shell": "Bash", "ask": "AskUserQuestion" },
  "fields": { "last_assistant_message": "response", "session_id": "conversation" },
  "notificationTypes": { "approval": "permission_prompt" },
  "resume": { "session": ["--resume", "{id}"], "continue": ["--continue"] },
  "sessions": { "command": ["mycli", "sessions", "--json", "{root}"] },
  "process": { "names": ["mycli"], "argvContains": ["/@acme/mycli/"] },
  "notes": "hooks only run in trusted folders"
}
```

The fields are:

- **`id`**: `[a-z0-9-]{1,32}`. It can't reuse a built-in or another plugin's id.
- **`program`**: the executable to resume or start.
- **`configDir`**: the plugin's hooks are installed by default only when this directory exists.
- **`hooks`**: the CLI must support command hooks in Claude Code's `{"hooks": {"<Event>": [{"matcher", "hooks": [{"type": "command", …}]}]}}` shape, with a JSON payload on stdin.
  - `settingsPath` is the file am writes the hooks into.
  - `projectDirVar` is the env var the CLI sets to the session directory.
  - `matcher`, `timeoutMs` and `name` are optional.
- **`events`**: maps native event names to canonical ones (`SessionStart`, `SessionEnd`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `Stop`, `StopFailure`, `Notification`, `PermissionRequest`, `SubagentStart`, `SubagentStop`). Hooks are installed for exactly these keys, in this order.
- **`tools`**: maps native tool names to Claude's (`Bash`, `Read`, `Edit`, `AskUserQuestion`, `EnterPlanMode`, `ExitPlanMode`, `Agent`…), so the waiting and planning rules apply.
- **`fields`**: renames canonical payload keys (`session_id`, `transcript_path`, `cwd`, `tool_name`, `tool_input`, `last_assistant_message`, `message`, `notification_type`, `stop_hook_active`, `permission_mode`, `agent_id`, `agent_type`…) to the CLI's own. Keys you don't list are read as-is. A CLI that sends `agent_id` and `agent_type` gets subagent tracking for free.
- **`notificationTypes`**: maps native notification types to canonical ones. Map whatever means "the user is being asked" to `permission_prompt`.
- **`resume`**: `{id}` is replaced by the session id. `continue` holds the arguments for "latest session".
- **`sessions.command`** (optional): prints a JSON array of `{"id", "cwd", "modified"}` (modified is unix seconds) for `{root}`. It is killed after 3 s. Without it, am resumes with `resume.continue`.
- **`process`**: matches the CLI's processes so they are excluded from "running processes".
- **`notes`** (optional): a caveat shown next to the hook status.

Invalid files are skipped and reported on the plugin page and in `am doctor`. Then turn the plugin on (it is on by default) and install its hooks from **Settings → Agent plugins**, or with `am hooks install --agent mycli`.

### By training mode (let the agent write it)

An agent CLI that am doesn't support yet can write and test its own manifest. Plan and design notes: `docs/plans/plugin-training-mode.md`.

1. **Start.** Settings → Agent plugins → `T` (or **+ Train a new harness plugin…**), then type the command that starts the CLI. Without the TUI, use `am harness train --command <cmd>`. am:
   - creates `~/.agent-monitor/training/<name>/`, a small git repo, so the training session appears under "Other sessions";
   - writes the prompt (`crates/am-core/src/harness/training/create.md`) to `TRAINING.md` there;
   - copies a one-line "read TRAINING.md" to the clipboard;
   - opens the CLI there in a new terminal.
2. **Listen.** The agent writes `manifest.json` with `"status": "draft"`. It lists every event its CLI documents: the ones it can map in `events`, the rest in `training.captureEvents`. It then runs `am harness add`. While a plugin is a draft, `am-hook` saves every event it receives to `hook-captures/<id>/`, under its native name, with the hook's argv, cwd and environment variable names, plus the values of variables that name a directory (this is how the agent finds `projectDirVar`). Events that aren't mapped are recorded but never change a status. Most CLIs only read hooks at startup, so the agent fires events from a new session: a short headless run of itself, or steps it gives the user.
3. **Iterate.**
   - `am harness captures <id>` summarises what fired.
   - `am harness test <id> --manifest ./manifest.json` replays the captures through a candidate manifest and prints each event's canonical name and status transition. It writes nothing.
   - `am harness validate` lists every problem with a JSON path and a hint. It refuses a hook file that another plugin already writes to.
   - `am harness add --replace` installs a new version and keeps the old one for `am harness rollback`.
4. **Use the draft.** Rows show a yellow `✎ draft` badge. Every manifest plugin, draft or published, records what goes wrong in `plugin_issues`: unmapped events or tools, a missing `session_id`/`cwd`, no directory for the session at all, a payload that isn't JSON, hooks for an id that has no loadable manifest, and a sessions command that fails. The first occurrence of each is also logged as a WARN under `plugin:<id>`, and draft events are logged at INFO. See them with `am harness issues <id>` and `am logs --module plugin:<id>`.
5. **Retrain or publish.** `e` on the plugin's row puts it back into draft and opens a session with `edit.md`, which points the agent at the manifest, the captures and the issues. `p` publishes the plugin after a confirmation: it needs a clean validation, removes the capture-only hooks, and drops the badge. The agent is told never to publish.

### As a built-in (when a manifest isn't enough)

1. Add a module in `crates/am-core/src/harness/` that implements `Harness`. Start from a manifest, as `gemini.rs` does, if most of the agent is declarative.
2. Add one entry to `harness::BUILTIN`. Nothing else in am changes.
3. Give it unit tests next to the module: normalisation, a full turn through `map_event_to_status`, the hook entry shape, and session discovery against a temp dir. Add an `am-hook` integration test that replays native payloads.
4. Update this file, `README.md`, `CLAUDE.md` and `STATUS_MAPPING.md`.
