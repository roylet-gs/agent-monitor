# am harness plugin manifest

A plugin is one JSON file, `<app dir>/agents/<id>.json`. Add it with `am harness add <file>` (which
validates it, keeps the previous version and installs its hooks); don't copy it in by hand.

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
  "status": "draft",
  "training": { "captureEvents": ["BeforeModel"], "promptArgs": ["-i", "{prompt}"],
                "verifiedWith": "mycli 1.2.3", "notes": "AfterTool has no tool_input on errors" }
}
```

## How am uses it

The harness runs `"<am-hook>" hook-event --agent <id> --worktree "$<projectDirVar>" --event <native>`
for every key of `events`, with the hook's JSON payload on stdin. am renames the payload's keys
(`fields`), maps the native event (`events`) and tool (`tools`) names onto Claude Code's vocabulary,
and runs its status rules: `UserPromptSubmit` → executing, `PreToolUse`/`PostToolUse` → executing,
`AskUserQuestion`/`ExitPlanMode` tool or a `permission_prompt` notification or `PermissionRequest` →
waiting, `Stop` → done, `SessionStart`/`SessionEnd` → idle, `SubagentStart`/`SubagentStop` with
`agent_id` → delegating.

## Fields

- **`id`** (required): `[a-z0-9-]{1,32}`; can't reuse a built-in's or another plugin's id.
- **`name`** (required), **`shortName`**: shown in the TUI.
- **`program`** (required): the executable that starts and resumes sessions (one word, no args).
- **`configDir`**: the harness has been used here when this dir exists; hooks are installed by
  default only then.
- **`hooks`** (required): the harness must run shell-command hooks from a JSON settings file shaped
  like Claude Code's: `{"hooks": {"<Event>": [{"matcher": "*", "hooks": [{"type": "command",
  "command": "…", "timeout": 5000}]}]}}`.
  - `settingsPath` (required): the file am writes into; am keeps every entry that isn't its own. Two
    plugins can't share one.
  - `projectDirVar` (required): the env var the harness sets to the session's project directory.
  - `matcher`, `timeoutMs` (default 5000), `name`: copied into each entry.
- **`events`** (required): native event name → canonical event: `SessionStart`, `SessionEnd`,
  `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `Stop`, `StopFailure`, `Notification`,
  `PermissionRequest`, `SubagentStart`, `SubagentStop`. Hooks are installed for exactly these keys.
- **`tools`**: native tool name → canonical tool (`Bash`, `Read`, `Write`, `Edit`, `Glob`, `Grep`,
  `WebFetch`, `WebSearch`, `AskUserQuestion`, `EnterPlanMode`, `ExitPlanMode`, `Agent`, …).
- **`fields`**: canonical payload key → the harness's key. Canonical keys: `hook_event_name`,
  `session_id`, `transcript_path`, `cwd`, `tool_name`, `tool_input`, `last_assistant_message`,
  `message`, `notification_type`, `stop_hook_active`, `permission_mode`, `agent_id`, `agent_type`,
  `agent_transcript_path`. Unlisted keys are read as-is.
- **`notificationTypes`**: native notification type → canonical; map "the user is being asked" to
  `permission_prompt`.
- **`resume`**: `session` args to resume a session (`{id}` = its id), `continue` args for the latest.
- **`sessions.command`**: argv that prints `[{"id", "cwd", "modified"}]` (unix seconds) for `{root}`;
  killed after 3 s. Without it, am resumes with `resume.continue`.
- **`process`**: `names` (process or script base names) and `argvContains` (substrings of the first
  argv entries) that identify the harness's processes.
- **`notes`**: a caveat shown next to the hook status.
- **`status`**: `draft` (being trained: every hook event is captured, rows show `✎ draft`) or
  `published` (default). `am harness add` makes a plugin a draft; the user publishes it.
- **`training`**:
  - `captureEvents`: extra native events hooked only while the plugin is a draft, so am records
    them (`am harness captures <id>`). They never change a status.
  - `promptArgs`: args that start the harness with an initial prompt (`{prompt}`), used when
    retraining.
  - `verifiedWith`, `notes`: what the plugin was checked against, and open findings.

## Commands

    am harness validate <file|id>     every problem, with a JSON path and a hint
    am harness add <file> [--replace] install as a draft (keeps the previous version)
    am harness captures <id> [--full] [--last N] [--event E]
    am harness test <id> [--manifest <file>]   replay captures through a manifest, no writes
    am harness issues <id> [--clear]  what went wrong while the plugin was used
    am harness show <id> [--json]
    am harness rollback <id>          restore the previous version
    am logs --module plugin:<id>      draft events and first occurrences of each issue
