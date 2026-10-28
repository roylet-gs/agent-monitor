# Agent Status Mapping

How hook events become an agent session's status, and a worktree's. The rules live in `crates/am-core/src/status.rs` (`map_event_to_status`, unit-tested there); the hook handler that applies them is `crates/am-hook/src/lib.rs`.

The rules are written against Claude Code's event vocabulary, which is am's **canonical** format. Other agent CLIs are translated into it by their provider (`crates/am-core/src/harness/`) before these rules run; see [Other agents](#other-agents) and `docs/agent-providers.md`.

## Status Types

| Status       | Glyph | Meaning |
| ------------ | ----- | ------- |
| *(none)*     | `·`   | No Claude session has reported for this worktree |
| `idle`       | `○`   | Session started or ended, or a turn ended without doing work; not waiting on the user |
| `executing`  | `●`   | Claude is running tools or generating |
| `planning`   | `◇`   | Claude is in plan mode (`permission_mode=plan`) |
| `waiting`    | `◐`   | Claude needs the user (permission prompt, question, plan approval) and hasn't finished its task |
| `delegating` | `◑` (green) | The main turn has ended but background subagents are still running |
| `done`       | `✓`   | Claude did work and finished; nothing needed from the user. Unlike `idle`, `done` means a turn actually executed |

## Processing Order (per hook event)

1. **Resolve the owner.** Walk up from `--worktree` to the directory containing `.git`. If it matches a tracked worktree, update `agent_status`; otherwise track it as a standalone session (`standalone_sessions`, shown under "Other sessions").
2. **Subagent bookkeeping** (always, even if the status won't change; see [Subagent Tracking](#subagent-tracking)).
3. **Count running subagents** for the owner *after* step 2. This `running` count is an input to the rules below, and is mirrored into `active_subagents` for 1.x compatibility.
4. **Map the event** with `map_event_to_status(event, current, running)`. `None` means "leave the status alone".
5. **Write.** If the status, `is_open` and `session_id` are unchanged and there's no new message, only `updated_at` is touched. Otherwise the row is upserted. `is_open` is false only after `SessionEnd`. `last_response` comes from `last_assistant_message` (main-agent events only, not subagent lifecycle) or a `Notification`'s `message`. Events from inside a subagent never write `session_id` or `last_response`.

## Event Origin

Claude Code tags tool events fired *inside* a subagent with `agent_id` / `agent_type`. am classifies each event by origin:

- **Main agent:** no `agent_id`.
- **Subagent lifecycle:** `SubagentStart` / `SubagentStop`. They carry `agent_id` but describe the subagent from the parent's side, so they count as main-agent events for status purposes.
- **From a subagent:** any other event with a non-empty `agent_id` (its `PreToolUse`, `PostToolUse`, `Notification`, …).

**Events from a subagent never move the parent's status**, with one exception. If the subagent needs the user, the worktree becomes `waiting`. "Needs the user" means:
- `PermissionRequest`
- `Notification` with `notification_type` `permission_prompt` or `elicitation_dialog`
- `PreToolUse` with `permission_prompt: true`

Everything else from a subagent → no change (it only updates that subagent's row).

## Done Status Protection

`done` is protected from late events. Background subagents can stop minutes after the parent's `Stop`, and informational events would otherwise drag `done` back to `executing` or `waiting`.

**When the current status is `done`, only these events are evaluated:**

| Event | Why |
| ----- | --- |
| `UserPromptSubmit` | User started a new task |
| `SessionStart` / `SessionEnd` | Session lifecycle |
| `Stop` / `StopFailure` | A new turn ended or failed |
| `PreToolUse` / `PostToolUse` (main agent) | Claude Code starts a new main turn **without** `UserPromptSubmit` when a background task notifies it, so main-agent tool activity is allowed through |
| `SubagentStart` with `running > 0` | A finished session's subagent was resumed (e.g. via SendMessage) → **`delegating`** directly |

**Everything else is ignored while `done`:** `SubagentStop`, `SubagentStart` when no subagent is running, `Notification` (including `idle_prompt` and permission prompts), `PermissionRequest`, and any other event. Subagent-origin events are already filtered out before this check.

## Delegating

While the status is `delegating` (the main turn ended with subagents still running):

| Event | Condition | Result |
| ----- | --------- | ------ |
| `SubagentStart` / `SubagentStop` | `running > 0` | `delegating` |
| `SubagentStart` / `SubagentStop` | `running == 0` (the last one stopped) | `done` |
| `Stop` | `stop_hook_active: true` | `waiting` |
| `Stop` | `running > 0` | `delegating` |
| `Stop` | `running == 0` | `done` |
| anything else | | falls through to the general rules below |

## Event → Status Mapping (general rules)

Applied in this order; the first match wins.

### Stop

| Condition | Result |
| --------- | ------ |
| `stop_hook_active: true` | `waiting` |
| `running > 0` | `delegating` |
| current status is `waiting` | `waiting` |
| `permission_mode == "plan"` or current status is `planning` | `waiting` |
| current status is `executing` | `done` |
| Fallback | `idle` |

### StopFailure

| Condition | Result |
| --------- | ------ |
| Always | `idle` |

### Notification

| Condition | Result |
| --------- | ------ |
| `notification_type` is `permission_prompt` or `elicitation_dialog` | `waiting` |
| `idle_prompt` and current status is `done` | No change |
| `idle_prompt` and `running > 0` | `delegating` |
| `idle_prompt` otherwise | `waiting` |
| Other notification types | No change |

### PermissionRequest

| Condition | Result |
| --------- | ------ |
| Always | `waiting` |

### SessionStart / SessionEnd

| Condition | Result |
| --------- | ------ |
| Always | `idle` (`SessionEnd` also sets `is_open = false`) |

### UserPromptSubmit

| Condition | Result |
| --------- | ------ |
| `permission_mode == "plan"` | `planning` |
| Otherwise | `executing` |

### Tool-specific

| Tool / Condition | Result |
| ---------------- | ------ |
| `tool_name` is `AskUserQuestion` | `waiting` |
| `tool_name` is `ExitPlanMode` | `waiting` |
| `tool_name` is `EnterPlanMode` | `planning` |
| `PreToolUse` with `permission_prompt: true` | `waiting` |

### Plan mode passthrough

| Condition | Result |
| --------- | ------ |
| `permission_mode == "plan"` (any remaining event) | `planning` |

### Tool / subagent activity

| Condition | Result |
| --------- | ------ |
| `PreToolUse` / `PostToolUse` / `SubagentStart` / `SubagentStop`, current status `planning` | `planning` |
| `PreToolUse` / `PostToolUse` / `SubagentStart` / `SubagentStop`, otherwise | `executing` |

### Fallback

| Condition | Result |
| --------- | ------ |
| No rule matched | `idle` |

## Subagent Tracking

Subagents are tracked individually in the `subagents` table, keyed by `(session_id, agent_id)`, instead of the 1.x `+1`/`−1` counter. The counter could be driven to 0 by duplicate or internal `SubagentStop`s while real subagents were still running, and a subagent's own `PreToolUse` could overwrite `delegating` with `executing`.

| Event | Effect on the `subagents` table |
| ----- | ------------------------------- |
| `SubagentStart` | Upsert the row as `running` (also when a finished subagent is resumed) |
| `SubagentStop` | Mark it `done` with `ended_at`, transcript path and last message (truncated to 500 chars). Repeats are idempotent; a stop for an unknown id creates a finished row |
| `PreToolUse` from a subagent | Record the tool name and a one-line summary of its input (Bash description or command, file path, search pattern, URL, …), bump the tool count, keep it `running` (a missed `SubagentStart` is recovered here) |
| `PostToolUse` from a subagent | Keeps it alive (refreshes `updated_at`) |
| `SessionStart` | Close running subagents of *other* sessions for this owner that are no longer open (a session that ended, or one from before schema v4). A second live session in the same worktree keeps its subagents |
| `SessionEnd` | Close running subagents of this session |

Rules:

- **Internal agents are ignored.** Agents with an empty `agent_type` (prompt suggestions, `/btw`, …) aren't user-visible work and never count as running.
- **Stale protection.** A `running` row only counts if it was updated within the last **2 hours**, so a missed `SubagentStop` (crash, killed session) can't pin a worktree on `delegating` forever.
- **Descriptions.** On the first event for a subagent, am reads Claude Code's `<session>/subagents/agent-<id>.meta.json` (next to the subagent transcript) for the Agent tool's `description` and `agentType`. This is best effort, since the file is undocumented.
- **Running count** = running, non-stale rows of the event's **session**: that is the `running` input to the rules above, so one session's subagents never make another `delegating`. The dashboard shows it as `◆ N subagents` on the session's row, or on the worktree row when it has one session. `agent_status.active_subagents` mirrors the owner-wide count for 1.x.
- **Parked subagents (dashboard only).** A background subagent that starts a long command (`run_in_background`) ends its turn while it waits, so Claude Code sends `SubagentStop`, and a fresh `SubagentStart` when the command finishes and wakes it. Its work isn't done in between. The TUI collector treats a subagent as **parked**, and so still running, when all of these hold: it belongs to one of the worktree's live sessions, it stopped within the last 2 hours, its last tool isn't `SubagentHandback` (the call that delivers a background subagent's final report), and a Claude Code shell command (`<shell> -c source ~/.claude/shell-snapshots/…`) is still running in the worktree. Parked subagents count in `◆ N subagents`, and a session (and worktree) that is `done` with one shows as `delegating`. This is display only: the DB keeps the hook-driven status, and the CLI doesn't apply it.

## Several Sessions per Worktree

Every event carrying a `session_id` updates that session's row in the `sessions` table (schema v4, keyed by `(agent, session_id)`): the rules above run against **that session's** previous status and subagent count. So two sessions in one worktree, from one harness or several, never overwrite each other. A `Stop` in one doesn't end the other's turn, and one session's tool calls don't hide another `waiting` on the user.

The owner's `agent_status` / `standalone_sessions` row is then rewritten as the **headline**: the most urgent open session (`waiting` > `executing` > `planning` > `delegating` > `done` > `idle`; the newest on a tie), or the newest session when none is open. Filters, sort, `am status` and 1.x read that row. An event without a `session_id` updates the owner's row directly, as before.

The dashboard lists a worktree's live sessions under its row when there are two or more. A session is live when it is effectively open (below). When the process scan sees fewer CLIs of a harness running in the worktree than that harness has open sessions, only the most recently active ones are kept, which drops sessions killed without `SessionEnd`. Closed sessions are pruned after a day.

## Open Sessions

`is_open` is true from the first event until `SessionEnd`. For filtering ("never hide a row with a live session"), for listing standalone sessions under "Other sessions", and for deciding whether `Enter` should resume a session, a session counts as **effectively open** only if `is_open` is set and it isn't `idle` with no events for **10 minutes**.

There is no display-time status override: the dashboard shows the stored status as is.

## Subscribed Hook Events

Installed into `~/.claude/settings.json` by `am hooks install`, each as `"<path>/am-hook" hook-event --worktree "$CLAUDE_PROJECT_DIR" --event <Event>`:

| Event | Purpose |
| ----- | ------- |
| `PreToolUse` | Tool execution start (main agent or subagent) |
| `PostToolUse` | Tool execution end |
| `Stop` | Turn completed |
| `StopFailure` | Turn failed (API error) |
| `Notification` | Permission prompts, elicitation dialogs, idle prompts |
| `SessionStart` | Session opened |
| `SessionEnd` | Session closed |
| `UserPromptSubmit` | User started a new task |
| `SubagentStart` | Subagent spawned or resumed |
| `SubagentStop` | Subagent completed |
| `PermissionRequest` | Claude Code 2.x managed permission prompt (installed with `--managed`) |

Set `AM_HOOK_CAPTURE=1` in Claude Code's environment to save the latest raw payload per event and origin to `~/.agent-monitor/hook-captures/<Event>-<main|subagent>.json`, which is useful for checking which fields Claude Code actually sends.

## Other agents

`am-hook hook-event --agent <id>` selects a provider that normalises the native payload first; no flag means Claude. The status row records the agent (`agent` column, NULL = Claude).

**Gemini CLI** (`--agent gemini`, hooks in `~/.gemini/settings.json`):

| Gemini event | Canonical event | Resulting status (via the rules above) |
| --- | --- | --- |
| `SessionStart` / `SessionEnd` | same | `idle` |
| `BeforeAgent` | `UserPromptSubmit` | `executing` |
| `BeforeTool` / `AfterTool` | `PreToolUse` / `PostToolUse` | `executing` (tool names mapped: `ask_user`/`exit_plan_mode` → `waiting`, `enter_plan_mode` → `planning`) |
| `Notification` (`ToolPermission`) | `Notification` (`permission_prompt`) | `waiting` |
| `AfterAgent` | `Stop` (`prompt_response` → last response) | `done` after work, else `idle` |

Gemini sends no subagent ids (a subagent is an `invoke_agent` tool call), so Gemini sessions never reach `delegating` and record nothing in `subagents`.

**Codex CLI** (`--agent codex`), **Qwen Code** (`--agent qwen`), **Factory Droid** (`--agent droid`) and **GitHub Copilot CLI** (`--agent copilot`) send Claude Code's event names and payload keys, so the rules above apply unchanged once tool names are mapped (Codex `apply_patch` → `Edit`; Qwen `run_shell_command` → `Bash`…; Droid `Execute` → `Bash`, `Create` → `Write`). Differences:

- Codex: `Interrupt` (the user stopped the turn) → `Stop`. No `Notification`; `waiting` comes from `PermissionRequest`. Subagent events carry `agent_id`, so `delegating` works.
- Droid: a cancelled turn sends `Notification` (`idle_prompt`) rather than `Stop`. No `SubagentStart`, so its subagents aren't tracked.
- Copilot: registered with PascalCase names, which select its Claude-shaped payload. Its permission and notification hooks only exist in the camelCase format, so only `AskUserQuestion` reaches `waiting`.

**Cursor** (`--agent cursor`, hooks in `~/.cursor/hooks.json`):

| Cursor event | Canonical event | Resulting status |
| --- | --- | --- |
| `sessionStart` / `sessionEnd` | same | `idle` |
| `beforeSubmitPrompt` | `UserPromptSubmit` | `executing` |
| `preToolUse` / `postToolUse` | `PreToolUse` / `PostToolUse` | `executing` (`Shell` → `Bash`) |
| `stop` | `Stop` | `done` after work, else `idle` |

The session is `conversation_id`, the directory `workspace_roots[0]`. Cursor has no permission hook, so it never shows `waiting`. Cursor also runs the user's Claude Code hooks; am-hook drops a `--agent claude` event whose payload has `cursor_version`, so a Cursor session is never counted as Claude.

**Campfire** (`--agent campfire`): Campfire has no command hooks. am's extension (`${CAMPFIRE_CODING_AGENT_DIR:-~/.campfire/agent}/extensions/agent-monitor-campfire.ts`) sends canonical event names itself, with `--worktree ""` and the session's directory as `cwd`, and only for the session's host (`CAMPFIRE_SESSION_ROLE=host`):

| Campfire event | Canonical event | Resulting status |
| --- | --- | --- |
| `session_start` | `SessionStart` | `idle` |
| `before_agent_start` | `UserPromptSubmit` (once per turn) | `executing` |
| observer `join.requested` / `permission.asked` | `Notification` `permission_prompt` | `waiting` (a `done` session is left alone) |
| `agent_end` | `Stop` (once per turn; `PostToolUse` first when the turn saw an ask) | `done` after work |
| `session_end` / `session_shutdown` | `SessionEnd` | `idle` |

There is no event for the host answering a join request or capability ask, so a `waiting` row from an ask between turns stays until the next prompt.
