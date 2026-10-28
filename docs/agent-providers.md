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

- **Settings → Harness plugins…** (TUI sub-page, `crates/am-tui/src/plugins_ui.rs`) lists every registered plugin with its source (`built-in` or the manifest file), whether it is on, its hook status and the default marker. It also shows manifest files that failed to load.
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
    fn is_present(&self) -> bool;           // config dir exists → install hooks by default (Embedded: + with_presence)
    fn settings_path(&self) -> PathBuf;     // ~/.gemini/settings.json
    fn hook_events(&self) -> Vec<String>;
    fn hook_entry(&self, event: &str, invocation: &str, native: bool) -> Value;
    fn hook_install(&self) -> HookInstall;  // default Json; TextBlock / OwnedFile (see "Hook install styles")
    fn place_hook_block(&self, text: &str, block: &str, ctx: &HookContext) -> io::Result<Option<String>>; // TextBlock: where the block goes; default None
    fn hook_companion(&self, ctx: Option<&HookContext>) -> io::Result<()>; // state beside the hook config; default nothing
    fn hook_command(&self, event: &str, invocation: &str) -> String; // full command line, for the text styles
    fn hook_timeout_ms(&self) -> u64;       // default 5000
    fn normalize(&self, payload: &Value, event_override: Option<&str>) -> HookEvent;
    fn discover_sessions(&self, root: &Path) -> Vec<Session>;
    fn resume_args(&self, session_id: Option<&str>, continue_latest: bool) -> Vec<String>; // (None, false) = fresh start
    fn context_usage(&self, root: &Path, session_id: &str) -> Option<ContextUsage>; // default None
    fn usage_files(&self, since: SystemTime) -> Vec<PathBuf>;           // default empty
    fn usage_record(&self, line: &str) -> Option<UsageRecord>;          // default None
    fn matches_process(&self, name: &str, argv: &[String]) -> bool;
}
```

`usage_files` and `usage_record` feed today's token total (`am_core::usage`). The first lists transcript files modified since local midnight, subagent transcripts included. The second parses one line into `{ id, at, cwd, tokens }`, with `tokens` holding each billed class apart (`TokenCounts`: input, 5-minute and 1-hour cache writes, cache reads, output) and an optional `cost_usd` (Claude prices it from `harness/claude/pricing.rs` by model id; `None` = unknown model, shown as unpriced). The tracker reads only appended bytes, de-duplicates by `id`, and attributes each `cwd` to the most specific worktree root. Claude implements both. Gemini and manifest plugins use the defaults for now, so their sessions don't count yet.

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
  "notes": "hooks only run in trusted folders",
  "mcp": { "file": "~/.mycli/settings.json", "key": "mcpServers" }
}
```

The fields are:

- **`id`**: `[a-z0-9-]{1,32}`. It can't reuse a built-in or another plugin's id.
- **`program`**: the executable to resume or start.
- **`configDir`**: the plugin's hooks are installed by default only when this directory exists.
- **`hooks`**: the CLI must run a command per hook event with a JSON payload on stdin, configured in a JSON file.
  - `settingsPath` is the file am writes the hooks into.
  - `projectDirVar` (optional) is the env var the CLI sets to the session directory. Without it the command passes `--worktree ""` and am-hook uses the payload's `cwd`.
  - `layout`: `nested` (default) writes Claude Code's `{"matcher", "hooks": [{"type": "command", "command", "timeout"}]}` per event; `flat` writes one `{"command", "timeout"}` object per entry (Cursor, Copilot).
  - `commandKey` / `timeoutKey` (default `command` / `timeout`) rename those keys; `timeoutUnit` is `ms` (default) or `s`; `entry` adds keys to every hook object (`{"type": "command"}`).
  - `file` adds top-level keys when they're missing, e.g. `{"version": 1}`. `hooksKey` (default `hooks`) is the key holding the events; `""` means the file itself is keyed by event (Factory Droid).
  - `matcher`, `timeoutMs` and `name` are optional.
- **`events`**: maps native event names to canonical ones (`SessionStart`, `SessionEnd`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `Stop`, `StopFailure`, `Notification`, `PermissionRequest`, `SubagentStart`, `SubagentStop`). Hooks are installed for exactly these keys, in this order. When nothing maps to `SessionEnd`, a session is never closed by a hook, so am-hook closes the owner's other settled (`done`/`idle`) sessions of that plugin whenever a new one sends `SessionStart` (they reopen on their next event).
- **`tools`**: maps native tool names to Claude's (`Bash`, `Read`, `Edit`, `AskUserQuestion`, `EnterPlanMode`, `ExitPlanMode`, `Agent`…), so the waiting and planning rules apply.
- **`fields`**: renames canonical payload keys (`session_id`, `transcript_path`, `cwd`, `tool_name`, `tool_input`, `last_assistant_message`, `message`, `notification_type`, `stop_hook_active`, `permission_mode`, `agent_id`, `agent_type`…) to the CLI's own. Keys you don't list are read as-is. A value starting with `/` is a JSON pointer into the payload (`"cwd": "/workspace_roots/0"`). A CLI that sends `agent_id` and `agent_type` gets subagent tracking for free.
- **`notificationTypes`**: maps native notification types to canonical ones. Map whatever means "the user is being asked" to `permission_prompt`.
- **`resume`**: `{id}` is replaced by the session id. `continue` holds the arguments for "latest session". `start` (optional, default none) holds the arguments a fresh session needs when the bare program doesn't start one: `acli` needs `["rovodev", "run"]`, Kiro `["chat", "--agent", "agent-monitor"]`. It is what am runs for a new session (also when continuing a CLI with no `continue` arguments) and what `am agent attach` suggests when there is nothing to resume.
- **`sessions.command`** (optional): prints a JSON array of `{"id", "cwd", "modified"}` (modified is unix seconds) for `{root}`. It is killed after 3 s. Without it, am resumes with `resume.continue`.
- **`process`**: matches the CLI's processes so they are excluded from "running processes".
- **`notes`** (optional): a caveat shown next to the hook status.
- **`mcp`** (optional): where the CLI reads MCP servers, so `am mcp install --agent mycli` can register am's MCP server. `file` is a JSON file with a `name → {"command", "args"}` map under `key` (default `mcpServers`); `entry` adds keys to am's server entry (`{"type": "stdio"}`). Built-ins: Claude Code (through `claude mcp add-json --scope user` when `claude` is installed, else `~/.claude.json`), Gemini CLI, Qwen Code and Cursor. Codex (TOML config), Factory Droid, Copilot CLI and Qoder CLI aren't covered yet.

Invalid files are skipped and reported on the plugin page and in `am doctor`. Then turn the plugin on (it is on by default) and install its hooks from **Settings → Harness plugins**, or with `am hooks install --agent mycli`.

### Hook install styles

Most CLIs read hooks from a JSON file, but not all. A built-in picks how am's hooks get into its CLI's config with `Harness::hook_install()` (an `Embedded` sets it with `.with_hook_install(f)`). The file is always `settings_path()` (the manifest's `hooks.settingsPath`). All three styles go through the same `install_hooks` / `uninstall_hooks` / `is_hooks_installed` / `dangling_hook_binaries` / `repair_dangling_hooks` / `check_hook_config` (`am doctor`) in `am_core::hooks_installer`, so nothing outside the harness module changes per style.

| Style | Use when | How am's hooks are recognised | Uninstall removes |
|---|---|---|---|
| `HookInstall::Json` (default) | The CLI reads hooks from a JSON file (Claude, Gemini, Codex, Qwen, Droid, Copilot, Cursor, manifest plugins) | Entries whose command matches `is_am_hook_command` under `hooks_key` | Only am's entries; everyone else's stay. A file that isn't a JSON object (comments) is refused |
| `HookInstall::TextBlock(TextBlock { comment, render })` | Hooks live in a TOML or YAML file (Kimi Code, Rovo Dev, Hermes) | The lines between `<comment> agent-monitor hooks <id> begin` and `… end` | Exactly the block (plus the blank line install put before it when it is last). Nothing outside the block is touched; a `begin` without `end` is an error, never half-replaced. am's commands outside the block (`hook-event --agent <id>`, e.g. after the CLI rewrote the file and dropped the markers) count as installed and refuse install and uninstall |
| `HookInstall::OwnedFile(OwnedFile { render, register })` | The CLI loads a JS/TS plugin or extension that must spawn am-hook (OpenCode, Pi, OMP, Campfire, Amp) | The file contains `OWNED_FILE_MARKER` (`agent-monitor-hook-plugin v1`) | The file, only while it holds the marker (a file of that name without it is never overwritten or deleted), and `register`'s JSON list value |

- **`render: fn(&HookContext) -> String`.** `HookContext` has `agent_id`, `program` (the absolute native `am-hook`, or `am` for the CLI fallback), `native`, `invocation` (`"/abs/am-hook" hook-event` or `cat | am hook-event`) and `hooks: Vec<HookCommand>`, one per `hook_events()` in order, each `{ event, command, timeout_ms }` with `timeout_secs()`. `command` is `hook_command()`: `{invocation} --agent <id> --worktree "$VAR"|"" --event <event>`. For a plugin that spawns without a shell, `ctx.argv(event, worktree)` gives `[program, "hook-event", "--agent", id, "--worktree", worktree, "--event", event]`; an empty worktree makes am-hook use the payload's `cwd`. The payload goes to the child's stdin.
- **Quoting:** `toml_string(s)` (TOML basic string), `yaml_string(s)` (YAML double-quoted scalar) and `js_string(s)` (a JSON string literal, valid JS/TS) quote a value, including a command with its own `"`. Don't hand-roll escaping; there is no toml/yaml dependency.
- **`TextBlock`** installs by replacing the existing block in place, or appending it after a blank line, creating the file and its directory when missing. Reinstalling is byte-for-byte idempotent. The marker lines may be indented (they are matched trimmed), so the block can sit inside a mapping: replacing it in place indents every line of the new block like the old `begin` line. A second `begin` line (nested, or a second block, e.g. after a dotfile merge) is an error, like a `begin` without its `end`, so install and uninstall refuse rather than leave a live copy.
- **Placing the block (`Harness::place_hook_block`)**: a YAML block that opens a top-level key the user's file already has (`hooks:`, `eventHooks:`) would make a duplicate key. Such a harness places the block itself:

  ```rust
  fn place_hook_block(&self, text: &str, block: &str, ctx: &HookContext) -> io::Result<Option<String>>;
  // Embedded: .with_block_placement(place), place: hooks_installer::PlaceBlock
  //   = fn(text: &str, block: &str, ctx: &HookContext) -> io::Result<Option<String>>
  ```

  `text` is the file without am's block (an earlier one is removed first, so every install re-places it); `block` is am's whole block (`block_text(comment, id, render(ctx))`: the `block_begin` line, the rendered body, the `block_end` line). Return `Some(new file text)`: nest the block under the user's key, re-indenting it or rendering the events again from `ctx` at the right indent, between `block_begin`/`block_end` lines (indent them too). Return `None` for the default (in place, else `append_block(text, block)`). Return an error to refuse a file am can't extend without editing the user's lines (`hooks: {}` inline, a clashing event); the install stops before anything is written and `am doctor` shows the message, since `check_hook_config` runs the placement too. A placed text that lost the marker lines is refused, so the hooks can always be found and removed again. Uninstall removes exactly the block wherever it sits. Rovo Dev (items under `eventHooks.events`, adding the keys outside the block when missing so a user's item after am's never loses its parent; it refuses a user's hook for one of its events, since Rovo runs only an event's first hook) and Hermes (under `hooks:`) use it.
- **State beside the config (`Harness::hook_companion`)**: some CLIs need more than the hook config, e.g. Hermes only runs a shell hook whose `(event, command)` pair is in `shell-hooks-allowlist.json`. `hook_companion(Some(ctx))` runs on install in every style, after the new config is worked out and just before it is written (an error stops the install with nothing written); `hook_companion(None)` runs after uninstall, also when there was nothing to remove. Embedded: `.with_hook_companion(f)`, `f: hooks_installer::HookCompanion = fn(Option<&HookContext>) -> io::Result<()>`.
- **`OwnedFile.register: Option<JsonListEntry { path, key, value }>`** is for a CLI that also needs the file listed in its JSON config (OpenCode's `"plugin": [...]`): `value` is added to the array at top-level `key` of `path` when missing, and removed on uninstall (the key goes with its last value). The JSON file is read like a hook file, so one am can't parse, or a `key` holding something other than a list, stops the install before anything is written (and `am doctor` shows it). Uninstall leaves the registration alone while the plugin file exists without the marker.
- **Dangling binaries:** for the text styles am scans the block (or owned file) for quoted absolute paths named `am-hook`/`am`/`am-rs` or followed by `hook-event`, in the same string or as the next element of an argv array (`["/abs/my-hook", "hook-event", …]`, also split across lines) (`hook_programs_in_text`), so `repair_dangling_hooks` and `am doctor` work for them too. Write the binary's path as its own quoted string (`ctx.program` via `js_string`, `ctx.argv(…)` rendered as a JS array, or the quoted `command`).

### As a built-in (when a manifest isn't enough)

1. Add a module in `crates/am-core/src/harness/` that implements `Harness`. If most of the agent is declarative, make it an `Embedded` (`embedded.rs`): a compiled-in manifest plus optional Rust session discovery, as `gemini.rs` and `codex.rs` do. Without discovery, resume offers the sessions am's hooks recorded. A CLI whose hooks aren't in a JSON file adds `.with_hook_install(f)` with a [hook install style](#hook-install-styles). The other `Embedded` builders cover the rest, so a built-in doesn't need its own `Harness` impl wrapping an `Embedded`:

   | Builder | Signature | For |
   |---|---|---|
   | `.with_hook_file(f)` | `fn() -> (PathBuf, &'static str)` | the hook file and its hooks key, picked at runtime (`$KIRO_HOME/agents/…`, `$XDG_CONFIG_HOME/muse/settings.json`) |
   | `.with_presence(f)` | `fn() -> bool` | also present when `f()` (as well as when the manifest's `configDir` exists): a config dir an env var moves (`$OMP_AGENT_DIR`, `$GROK_HOME`), or `\|\| hook_file().0.parent().is_some_and(Path::is_dir)` |
   | `.with_normalize(f)` | `NormalizeFn = fn(&Manifest, &Value, Option<&str>) -> HookEvent` | a payload rule the manifest maps can't express: rewrite the payload before `manifest.normalize(payload, event)` (camelCase keys), adjust the event after it, or drop it (with `--worktree ""`, an event with no `cwd` is skipped) |
   | `.with_hook_install(f)` | `fn() -> HookInstall` | a non-JSON [hook install style](#hook-install-styles) |
   | `.with_block_placement(f)` | `PlaceBlock` | where a text block goes (`place_hook_block`) |
   | `.with_hook_companion(f)` | `HookCompanion` | state kept beside the hook config (`hook_companion`) |

   `paths::home_dir()` falls back to the passwd entry when `$HOME` is unset or empty (some CLIs run hooks with a scrubbed environment), so use it (or `harness::home_join`) rather than reading `$HOME`.
2. Add one entry to `harness::BUILTIN`. Nothing else in am changes.
3. Give it unit tests next to the module: normalisation, a full turn through `map_event_to_status`, the hook entry shape, and session discovery against a temp dir. Add an `am-hook` integration test that replays native payloads.
4. Update this file, `README.md`, `CLAUDE.md` and `STATUS_MAPPING.md`.

## Built-in harnesses and the survey behind them (Oct 2026)

Most coding-agent CLIs now copy Claude Code's hook design: a command per event, JSON on stdin, Claude-style event names and `session_id`/`cwd`/`tool_name` keys. That's why the canonical event stays Claude's vocabulary, and why each built-in below is a manifest (plus Rust discovery for Codex and Grok, and a runtime hook-file choice and child-session filter for Muse). The details come from each tool's official hook docs.

| Harness | Hook file and shape | Payload notes | Waiting | Resume |
|---|---|---|---|---|
| Codex CLI (`codex`) | `~/.codex/hooks.json`, nested, timeout in s | Claude keys; shell = `Bash`, edits = `apply_patch`; no project-dir variable (payload `cwd`); `Interrupt` ends a turn | `PermissionRequest` | `codex resume <id>` / `--last`; rollouts `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` |
| Qwen Code (`qwen`) | `~/.qwen/settings.json`, nested, s, `$QWEN_PROJECT_DIR` | Claude keys, runtime tool ids (`run_shell_command`…), subagent events | `Notification` `permission_prompt`, `PermissionRequest` | `qwen --resume <id>` / `--continue` |
| Factory Droid (`droid`) | `~/.factory/hooks.json` keyed by event at the top level (or the `hooks` key of `settings.json` when `hooks.json` doesn't exist), nested, s, `$FACTORY_PROJECT_DIR` | Claude keys; tools `Execute`/`Create`/`ApplyPatch`; a cancelled turn sends `idle_prompt` instead of `Stop` | `Notification` `permission_prompt` | `droid --resume <id>` |
| GitHub Copilot CLI (`copilot`) | `~/.copilot/hooks/agent-monitor.json` (am owns it), flat, `version: 1`, `timeoutSec` | PascalCase event names select the Claude-shaped payload with Claude tool names; no project-dir variable | questions only (its permission/notification hooks exist only in the camelCase format) | `copilot --resume <id>` / `--continue` |
| Cursor (`cursor`) | `~/.cursor/hooks.json`, flat, `version: 1`, s, `$CURSOR_PROJECT_DIR` | camelCase events; `conversation_id` and `workspace_roots[]` (JSON pointers); `Shell` = `Bash`. Cursor also runs `~/.claude/settings.json` hooks, so am-hook drops Claude events that carry `cursor_version` | none (no permission hook) | `cursor-agent --resume <id>` |
| Qoder CLI (`qoder`) | `~/.qoder/settings.json` (`$QODER_CONFIG_DIR` not followed), nested, s (default 600), `$QODER_PROJECT_DIR` | Claude keys and Claude tool names; subagent, `PermissionRequest` and `StopFailure` events; binary `qodercli` (npm `@qoder-ai/qodercli`, which also ships a `qoder` dispatcher). The Qoder IDE and JetBrains plugin run the same hooks without `$QODER_PROJECT_DIR` (so `--worktree ""` → payload `cwd`; no `cwd` → dropped), recorded as `qoder`; `qodercli --resume` may not find their sessions. The IDE needs a restart after install, the CLI reloads live | `PermissionRequest`, `Notification` | `qodercli --resume <id>` / `-c` |
| CodeBuddy Code (`codebuddy`) | `~/.codebuddy/settings.json` (`$CODEBUDDY_CONFIG_DIR/settings.json` when set), nested, s, `$CODEBUDDY_PROJECT_DIR` | Claude keys, event names, tool names and subagent `agent_id`s (so `delegating` works); hook edits are reviewed in `/hooks` before they apply | `Notification` `permission_prompt`, `PermissionRequest` | `codebuddy --resume <id>` / `--continue` |
| Antigravity CLI (`antigravity`) | `~/.gemini/config/hooks.json` (not Gemini CLI's `settings.json`) under a named group, `hooksKey: "agent-monitor"` (others' groups, e.g. cmux's `cmux`, are kept), flat `{"type": "command", "command", "timeout"}`, s; only `SessionStart`, `PreInvocation`, `Stop` (the tool events need a `{"matcher", "hooks"}` wrapper and aren't needed) | camelCase: `conversationId`, `workspacePaths[0]` (JSON pointer), `transcriptPath`; no project-dir variable. `PreInvocation` fires per model invocation. `Stop` with `fullyIdle: false` (background work) is read as activity by a `with_normalize` pass (`antigravity.rs`). Presence = `~/.gemini/antigravity-cli` | none (no permission or notification hook) | `agy --conversation <id>`; conversations in `~/.gemini/antigravity-cli/brain/<id>/` carry no cwd, so resume offers the sessions am's hooks recorded; no session-end hook, so a new conversation closes the settled ones; process `agy` or `antigravity` |
| Grok CLI (`grok`) | `~/.grok/hooks/agent-monitor.json` (`$GROK_HOME/hooks/`; am owns it), nested, s | camelCase keys (`sessionId`, `notificationType`, `toolName`), read through a snake_case pre-pass in `grok.rs`; Grok strips the hook's env, so no project-dir variable (payload `cwd`); `SessionEnd` fires every turn, so it isn't hooked (`Stop` ends the turn, a `waiting` one too: `stop_ends_wait`); no `PostToolUse`/subagent events | `Notification` `permission_prompt`, except the generic "Tool permission requested" Grok sends for every tool step | `grok -r <id>`; sessions `~/.grok/sessions/<percent-encoded cwd>/<id>/chat_history.jsonl` |
| Muse Code (`muse`) | `$XDG_CONFIG_HOME/muse/settings.json` or `~/.config/muse/settings.json` (picked at runtime), nested, s, `schema_version: 1` added when missing (Muse won't start without it) | Claude event names and keys; only `SessionStart`, `UserPromptSubmit`, `Notification`, `Stop`, `StopFailure`, `SessionEnd` (no tool hooks); no project-dir variable (payload `cwd`); internal child sessions (`turn_id == session_id`) are ignored; hooks run through the login `$SHELL -c` with a scrubbed environment | `Notification` `permission_prompt` (unverified) | `muse resume <id>`; sessions am recorded (Muse's `session-index.db` schema isn't documented) |
| Kiro CLI (`kiro`) | the agent file `~/.kiro/agents/agent-monitor.json` (`$KIRO_HOME/agents/`), which am owns: flat, `timeout_ms`, plus `name`, `description` and `tools: ["*"]` | camelCase events (`agentSpawn`, `userPromptSubmit`, `preToolUse`, `postToolUse`, `stop`); Claude keys (`session_id`, `cwd`, `tool_name`, `tool_input`); tools `execute_bash`/`fs_read`/`fs_write` (v3 aliases `shell`/`read`/`write`); no project-dir variable (payload `cwd`). **Hooks run only while the `agent-monitor` agent is in use** | none (no permission hook) | `kiro-cli chat --agent agent-monitor --resume-id <id>` / `--resume` |
| Rovo Dev CLI (`rovodev`) | `~/.rovodev/config.yml`, a YAML `TextBlock` of `- name: <event>` / `commands: [- command: …]` items in `eventHooks.events` (the keys added outside the block when missing, dropped on uninstall once empty); an inline `eventHooks: {…}` or `events: []`, or a user's item for one of am's events (Rovo runs only an event's first hook), is refused with a message; no timeout; `--worktree "$PWD"` | snake_case events (`on_user_prompt`, `on_tool_start`/`_end`, `on_tool_permission`, `on_complete`, `on_error` → `StopFailure`, `on_session_start`/`_end`); the stdin payload is `{session_id, transcript_path, cwd, timestamp, hook_event_name, attributes}` (202608.25.1), so sessions get their own rows (older releases without `session_id` update the worktree row directly); the tool is `attributes.tool_input` (`tool_name`, `args`), a flat `attributes.tool_name` (`on_tool_permission`) or the first of `attributes.tool_calls[]` (`on_tool_start`) | `on_tool_permission` → `PermissionRequest` | `acli rovodev run --restore <id>` / `--restore`; sessions `~/.rovodev/sessions/<id>/metadata.json` (workspace path; `sessions.persistenceDir` moves the dir); a fresh session is `acli rovodev run` (manifest `resume.start`) |
| Kimi Code (`kimi`) | `[[hooks]]` tables (`event`, `command`, `timeout` in s) in a marked block (`HookInstall::TextBlock`) of `${KIMI_CODE_HOME:-~/.kimi-code}/config.toml` only (the legacy Kimi CLI ≤ 1.49's `~/.kimi/config.toml` is never written: its closed event list has no `PermissionRequest`/`PermissionResult`/`Interrupt`, so it would fail to start) | Claude keys and tool names; no project-dir variable (payload `cwd`); `PermissionResult` and `PostToolUseFailure` → `PostToolUse`; a cancelled turn fires `Interrupt` instead of `Stop` (→ `StopFailure`, idle); `Notification` is background tasks only, not hooked; subagent events carry no id | `PermissionRequest` | `kimi --session <id>` / `--continue` |

### Kiro CLI

Kiro has two hook formats: 2.x keeps hooks inside an *agent configuration file* as an object keyed by camelCase trigger, and v3 adds standalone `.kiro/hooks/*.json` files (global ones in `~/.kiro/hooks/`) holding an *array* of `{name, trigger, action: {type, command}, timeout}`. The array form doesn't fit the event-keyed hook files am's installer writes, but v3 still accepts the 2.x object form as compatibility input, so am uses that one for both. That resolves the earlier "two formats" objection; the other one, no waiting signal, simply means a Kiro session shows `executing`/`done` but never `waiting`.

Because the hooks belong to an agent, am writes a whole agent, `agent-monitor`, and Kiro runs its hooks only while that agent is in use. Start Kiro with `kiro-cli chat --agent agent-monitor`, or make it the default with `kiro-cli settings chat.defaultAgent agent-monitor`. When am starts or resumes Kiro itself (`Enter`, `am agent attach`), it always passes `chat --agent agent-monitor`; the manifest's `resume.start` covers a fresh start. `kiro.rs` adds the hook file under `$KIRO_HOME` (`with_hook_file`) and a presence check (`with_presence`): `$KIRO_HOME` (or `~/.kiro`) must exist *and* `kiro-cli` must be on `$PATH` or in `~/.local/bin`, because the Kiro IDE creates `~/.kiro` too and its users would otherwise get hooks nothing runs (the manifest has no `configDir` for the same reason). The agent gets `tools: ["*"]`: a custom agent without `tools` has none, so the model couldn't run anything and the tool hooks would never fire (as cmux found against kiro-cli 2.5.0). It also gets `includeMcpJson: true`: Kiro's default agent loads the MCP servers in `~/.kiro/settings/mcp.json` (and the workspace's), but a custom agent only does when it opts in, so without it a session started as `agent-monitor` would lose the user's MCP tools. `includeMcpJson` is the key Kiro documents for both CLI 2.x and v3; Amazon Q CLI's agent format called it `useLegacyMcpJson` and rejects unknown keys, but am writes this file only for `kiro-cli`. Like every other file default, these are only added when missing, so a user's own `tools` list and MCP choice, and their own hooks in the file, are kept. If you'd rather keep your own agent, copy the `hooks` block from `agent-monitor.json` into it. Exit 2 from a `preToolUse` hook blocks the tool (its stderr goes to the agent), any other non-zero exit shows stderr as a warning, and stdout is captured; am-hook always exits 0 and prints nothing.

Kiro keeps conversations in its own SQLite store, so resume offers the sessions am's hooks recorded. The process scan matches `kiro-cli` and its `kiro-cli-chat` child.

Verified against Kiro's hook docs (kiro.dev/docs/hooks, `/docs/cli/v3/hooks-migration/`) and cmux's installer (`manaflow-ai/cmux`, `CLI/CMUXCLI+AgentHookCatalog.swift`); not yet run against a live `kiro-cli`. Still unconfirmed: the `stop` payload (whether it carries the assistant's reply, so am shows no last response yet), the tool names v3 sends, whether `--agent` together with `--resume-id` keeps the resumed conversation on the `agent-monitor` agent, and that a live `kiro-cli` (`/agent schema`) takes `includeMcpJson` (it is in Kiro's agent configuration reference). `AM_HOOK_CAPTURE=1` saves real payloads to check.

Not built in, and why:

- **JetBrains Junie CLI**: Claude-shaped hooks in `~/.junie/config.json`, but they're Early Access, there's no project-dir variable, and `Stop` carries no `session_id` or `cwd`, so the end of a turn can't be attributed.
- **Augment Auggie CLI**: hook `command` must be the path of a `.sh` script (no arguments), and payloads use `conversation_id`/`workspace_roots`. It would need a generated wrapper script per install.
- **Goose**: hooks exist only inside plugin directories, and the payload names the event `event`.
- **Windsurf / Devin Desktop**: no session, stop or permission events; the only turn end is the async `post_cascade_response`.
- **Crush**: `PreToolUse` only.
- **Cline**: one executable file per hook, and current docs point to in-process SDK plugins.
- **OpenCode, Amp**: JS/TS plugins only; supporting them means shipping a small bridge plugin that runs `am-hook`.
- **Aider**: no hooks, only a `--notifications-command` with no payload.
- **Mistral Vibe**: TOML config with tool and `post_agent` events only.

Any of these can still be added as a manifest where its hooks fit the format above.

### Muse Code notes

Sources: cmux's Muse integration (`manaflow-ai/cmux`: `CLI/CMUXCLI+AgentHookCatalog.swift`, `CLI/cmux.swift`, `docs/agent-hooks.md`, `AgentResumeArgv.swift`) and the third-party `develate/musecode-cli-php` SDK (settings path, `schema_version`). No official Muse hook reference was found, so:

- **Notification types are unverified.** am assumes Claude's `notification_type` values; anything else leaves the status unchanged. Capture real payloads with `AM_HOOK_CAPTURE=1` (in the environment Muse passes to hooks) and add a `notificationTypes` map if they differ.
- **Child sessions.** Muse runs reminders and delegated subagents as internal child sessions whose hooks reuse the session id as the turn id (cmux ignores them). am ignores them too (a `with_normalize` pass turns them into an `ignored` event): they'd otherwise be counted as extra sessions in the worktree and, being newest, could displace the real one when the collector caps sessions at the number of running `muse` processes. Muse gives no parent id, so they can't be shown as subagents. A child blocked on the user (`permission_prompt`/`elicitation_dialog` notification) is kept without its session id, so am-hook marks the owner's row `waiting` directly. Lifecycle events carry no `turn_id` at all, so a child's `SessionStart` (if Muse fires one; unverified) would look top-level: Muse's `SessionStart` is dropped (a session appears with its first prompt), and am-hook skips a `SessionEnd` for a session it never recorded.
- **Shell.** Muse runs hooks through the user's login `$SHELL -c` with a scrubbed environment. am's command is `"/abs/am-hook" hook-event --agent muse --worktree "" --event X`, which sh, bash, zsh and fish run unchanged; nushell needs `^"/abs/am-hook" …`, so nushell users edit the entries by hand. cmux writes `#!/bin/sh` wrapper scripts instead; am doesn't, to keep one install path for every harness. am-hook falls back to the passwd entry for its home directory if `$HOME` is missing.
- **No tool hooks.** cmux installs none for Muse, so am doesn't either. Without `PreToolUse`/`PostToolUse`, nothing says a permission prompt was answered, so Muse's events carry `HookEvent::stop_ends_wait` (shared with Grok) and its `Stop` after `waiting` reads `done` (the done cue plays) instead of staying `waiting` as for Claude.
