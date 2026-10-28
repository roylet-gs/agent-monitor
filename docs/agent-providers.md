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
    fn is_present(&self) -> bool;           // config dir exists → install hooks by default
    fn settings_path(&self) -> PathBuf;     // ~/.gemini/settings.json
    fn hook_events(&self) -> Vec<String>;
    fn hook_entry(&self, event: &str, invocation: &str, native: bool) -> Value;
    fn hook_install(&self) -> HookInstall;  // default Json; TextBlock / OwnedFile (see "Hook install styles")
    fn hook_command(&self, event: &str, invocation: &str) -> String; // full command line, for the text styles
    fn hook_timeout_ms(&self) -> u64;       // default 5000
    fn normalize(&self, payload: &Value, event_override: Option<&str>) -> HookEvent;
    fn discover_sessions(&self, root: &Path) -> Vec<Session>;
    fn resume_args(&self, session_id: Option<&str>, continue_latest: bool) -> Vec<String>;
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
- **`events`**: maps native event names to canonical ones (`SessionStart`, `SessionEnd`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `Stop`, `StopFailure`, `Notification`, `PermissionRequest`, `SubagentStart`, `SubagentStop`). Hooks are installed for exactly these keys, in this order.
- **`tools`**: maps native tool names to Claude's (`Bash`, `Read`, `Edit`, `AskUserQuestion`, `EnterPlanMode`, `ExitPlanMode`, `Agent`…), so the waiting and planning rules apply.
- **`fields`**: renames canonical payload keys (`session_id`, `transcript_path`, `cwd`, `tool_name`, `tool_input`, `last_assistant_message`, `message`, `notification_type`, `stop_hook_active`, `permission_mode`, `agent_id`, `agent_type`…) to the CLI's own. Keys you don't list are read as-is. A value starting with `/` is a JSON pointer into the payload (`"cwd": "/workspace_roots/0"`). A CLI that sends `agent_id` and `agent_type` gets subagent tracking for free.
- **`notificationTypes`**: maps native notification types to canonical ones. Map whatever means "the user is being asked" to `permission_prompt`.
- **`resume`**: `{id}` is replaced by the session id. `continue` holds the arguments for "latest session".
- **`sessions.command`** (optional): prints a JSON array of `{"id", "cwd", "modified"}` (modified is unix seconds) for `{root}`. It is killed after 3 s. Without it, am resumes with `resume.continue`.
- **`process`**: matches the CLI's processes so they are excluded from "running processes".
- **`notes`** (optional): a caveat shown next to the hook status.
- **`mcp`** (optional): where the CLI reads MCP servers, so `am mcp install --agent mycli` can register am's MCP server. `file` is a JSON file with a `name → {"command", "args"}` map under `key` (default `mcpServers`); `entry` adds keys to am's server entry (`{"type": "stdio"}`). Built-ins: Claude Code (through `claude mcp add-json --scope user` when `claude` is installed, else `~/.claude.json`), Gemini CLI, Qwen Code and Cursor. Codex (TOML config), Factory Droid and Copilot CLI aren't covered yet.

Invalid files are skipped and reported on the plugin page and in `am doctor`. Then turn the plugin on (it is on by default) and install its hooks from **Settings → Harness plugins**, or with `am hooks install --agent mycli`.

### Hook install styles

Most CLIs read hooks from a JSON file, but not all. A built-in picks how am's hooks get into its CLI's config with `Harness::hook_install()` (an `Embedded` sets it with `.with_hook_install(f)`). The file is always `settings_path()` (the manifest's `hooks.settingsPath`). All three styles go through the same `install_hooks` / `uninstall_hooks` / `is_hooks_installed` / `dangling_hook_binaries` / `repair_dangling_hooks` / `check_hook_config` (`am doctor`) in `am_core::hooks_installer`, so nothing outside the harness module changes per style.

| Style | Use when | How am's hooks are recognised | Uninstall removes |
|---|---|---|---|
| `HookInstall::Json` (default) | The CLI reads hooks from a JSON file (Claude, Gemini, Codex, Qwen, Droid, Copilot, Cursor, manifest plugins) | Entries whose command matches `is_am_hook_command` under `hooks_key` | Only am's entries; everyone else's stay. A file that isn't a JSON object (comments) is refused |
| `HookInstall::TextBlock(TextBlock { comment, render, place, companion })` | Hooks live in a TOML or YAML file (Kimi Code, Rovo Dev, Hermes) | The lines between `<comment> agent-monitor hooks <id> begin` and `… end` | Exactly the block (plus the blank line install put before it when it is last). Nothing outside the block is touched; a `begin` without `end` is an error, never half-replaced |
| `HookInstall::OwnedFile(OwnedFile { render, register })` | The CLI loads a JS/TS plugin or extension that must spawn am-hook (OpenCode, Pi, OMP, Campfire, Amp) | The file contains `OWNED_FILE_MARKER` (`agent-monitor-hook-plugin v1`) | The file, only while it holds the marker (a file of that name without it is never overwritten or deleted), and `register`'s JSON list value |

- **`render: fn(&HookContext) -> String`.** `HookContext` has `agent_id`, `program` (the absolute native `am-hook`, or `am` for the CLI fallback), `native`, `invocation` (`"/abs/am-hook" hook-event` or `cat | am hook-event`) and `hooks: Vec<HookCommand>`, one per `hook_events()` in order, each `{ event, command, timeout_ms }` with `timeout_secs()`. `command` is `hook_command()`: `{invocation} --agent <id> --worktree "$VAR"|"" --event <event>`. For a plugin that spawns without a shell, `ctx.argv(event, worktree)` gives `[program, "hook-event", "--agent", id, "--worktree", worktree, "--event", event]`; an empty worktree makes am-hook use the payload's `cwd`. The payload goes to the child's stdin.
- **Quoting:** `toml_string(s)` (TOML basic string), `yaml_string(s)` (YAML double-quoted scalar) and `js_string(s)` (a JSON string literal, valid JS/TS) quote a value, including a command with its own `"`. Don't hand-roll escaping; there is no toml/yaml dependency.
- **`TextBlock`** installs by replacing the existing block in place, or appending it after a blank line, creating the file and its directory when missing. Reinstalling is byte-for-byte idempotent. The block must be valid where it is appended: a YAML block that opens a top-level key the user's file already has (say `hooks:`) makes a duplicate key, so such a CLI needs a key it owns or a `place`.
- **`TextBlock.place: Option<fn(text_without_block, block) -> io::Result<String>>`** decides where the whole block (marker lines included) goes; every install then removes the old block and places it again, and an `Err` stops the install before anything is written. Hermes uses it to nest its block under the user's own `hooks:` key (`harness::hermes::place`, which also shows `append_block`, the default). Markers may be indented: they are matched trimmed.
- **`TextBlock.companion: Option<fn(Option<&HookContext>) -> io::Result<()>>`** keeps state the CLI holds beside its config in step: it runs with the hooks just before the block is written (an error stops the install), and with `None` after the uninstall removed it. Hermes's allowlist of approved hook commands is one (`harness::hermes::update_allowlist`).
- **`OwnedFile.register: Option<JsonListEntry { path, key, value }>`** is for a CLI that also needs the file listed in its JSON config (OpenCode's `"plugin": [...]`): `value` is added to the array at top-level `key` of `path` when missing, and removed on uninstall (the key goes with its last value). The JSON file is read like a hook file, so one am can't parse stops the install before anything is written.
- **Dangling binaries:** for the text styles am scans the block (or owned file) for quoted absolute paths named `am-hook`/`am`/`am-rs` or followed by `hook-event` (`hook_programs_in_text`), so `repair_dangling_hooks` and `am doctor` work for them too. Write the binary's path as its own quoted string (`ctx.program` via `js_string`, or the quoted `command`).

### As a built-in (when a manifest isn't enough)

1. Add a module in `crates/am-core/src/harness/` that implements `Harness`. If most of the agent is declarative, make it an `Embedded` (`embedded.rs`): a compiled-in manifest plus optional Rust session discovery, as `gemini.rs` and `codex.rs` do. Without discovery, resume offers the sessions am's hooks recorded. A CLI whose hooks aren't in a JSON file adds `.with_hook_install(f)` with a [hook install style](#hook-install-styles).
2. Add one entry to `harness::BUILTIN`. Nothing else in am changes.
3. Give it unit tests next to the module: normalisation, a full turn through `map_event_to_status`, the hook entry shape, and session discovery against a temp dir. Add an `am-hook` integration test that replays native payloads.
4. Update this file, `README.md`, `CLAUDE.md` and `STATUS_MAPPING.md`.

## Built-in harnesses and the survey behind them (Oct 2026)

Most coding-agent CLIs now copy Claude Code's hook design: a command per event, JSON on stdin, Claude-style event names and `session_id`/`cwd`/`tool_name` keys. That's why the canonical event stays Claude's vocabulary, and why each built-in below is a manifest (plus Rust discovery for Codex). The details come from each tool's official hook docs.

| Harness | Hook file and shape | Payload notes | Waiting | Resume |
|---|---|---|---|---|
| Codex CLI (`codex`) | `~/.codex/hooks.json`, nested, timeout in s | Claude keys; shell = `Bash`, edits = `apply_patch`; no project-dir variable (payload `cwd`); `Interrupt` ends a turn | `PermissionRequest` | `codex resume <id>` / `--last`; rollouts `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` |
| Qwen Code (`qwen`) | `~/.qwen/settings.json`, nested, s, `$QWEN_PROJECT_DIR` | Claude keys, runtime tool ids (`run_shell_command`…), subagent events | `Notification` `permission_prompt`, `PermissionRequest` | `qwen --resume <id>` / `--continue` |
| Factory Droid (`droid`) | `~/.factory/hooks.json` keyed by event at the top level (or the `hooks` key of `settings.json` when `hooks.json` doesn't exist), nested, s, `$FACTORY_PROJECT_DIR` | Claude keys; tools `Execute`/`Create`/`ApplyPatch`; a cancelled turn sends `idle_prompt` instead of `Stop` | `Notification` `permission_prompt` | `droid --resume <id>` |
| GitHub Copilot CLI (`copilot`) | `~/.copilot/hooks/agent-monitor.json` (am owns it), flat, `version: 1`, `timeoutSec` | PascalCase event names select the Claude-shaped payload with Claude tool names; no project-dir variable | questions only (its permission/notification hooks exist only in the camelCase format) | `copilot --resume <id>` / `--continue` |
| Cursor (`cursor`) | `~/.cursor/hooks.json`, flat, `version: 1`, s, `$CURSOR_PROJECT_DIR` | camelCase events; `conversation_id` and `workspace_roots[]` (JSON pointers); `Shell` = `Bash`. Cursor also runs `~/.claude/settings.json` hooks, so am-hook drops Claude events that carry `cursor_version` | none (no permission hook) | `cursor-agent --resume <id>` |
| Hermes Agent (`hermes`) | `$HERMES_HOME/config.yaml` (`~/.hermes`), a YAML [text block](#hook-install-styles) of shell hooks `{command, timeout}` (s), plus approvals in `shell-hooks-allowlist.json` | `{hook_event_name, tool_name, tool_input, session_id, cwd, profile, extra}`; no project-dir variable (payload `cwd`); commands run without a shell (`shlex`); `on_session_end` ends every turn, `on_session_finalize` the session; approval events have no `session_id` | `pre_approval_request`; `clarify` tool | `hermes --resume <id>` / `--continue` |

Not built in, and why:

- **JetBrains Junie CLI**: Claude-shaped hooks in `~/.junie/config.json`, but they're Early Access, there's no project-dir variable, and `Stop` carries no `session_id` or `cwd`, so the end of a turn can't be attributed.
- **Augment Auggie CLI**: hook `command` must be the path of a `.sh` script (no arguments), and payloads use `conversation_id`/`workspace_roots`. It would need a generated wrapper script per install.
- **Kiro CLI**: two hook formats (per-agent JSON in 2.x, `.kiro/hooks/*.json` in v3) and no waiting signal.
- **Goose**: hooks exist only inside plugin directories, and the payload names the event `event`.
- **Windsurf / Devin Desktop**: no session, stop or permission events; the only turn end is the async `post_cascade_response`.
- **Crush**: `PreToolUse` only.
- **Cline**: one executable file per hook, and current docs point to in-process SDK plugins.
- **OpenCode, Amp**: JS/TS plugins only; supporting them means shipping a small bridge plugin that runs `am-hook`.
- **Aider**: no hooks, only a `--notifications-command` with no payload.
- **Mistral Vibe**: TOML config with tool and `post_agent` events only.

Any of these can still be added as a manifest where its hooks fit the format above.
