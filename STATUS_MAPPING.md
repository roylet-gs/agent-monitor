# Agent Status Mapping

## Status Types

| Status      | Meaning                                                                                                                                                                                                   |
| ----------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `none`      | Session not running, no Claude session linked                                                                                                                                                             |
| `idle`      | Session ended / session is not asking any questions or waiting on any active user input                                                                                                                   |
| `executing` | Claude is actively running tools or generating                                                                                                                                                            |
| `planning`  | Claude is in plan mode (`permission_mode=plan`)                                                                                                                                                           |
| `waiting`   | Claude is waiting for user input and has not completed its task                                                                                                                                           |
| `done`      | Claude finished a task and requires no more user input (this is slightly different from idle as done means Claude did some work and is now done while idle means Claude has done nothing yet and is idle) |

## Done Status Protection

The `done` status is protected from late-arriving events. Background subagents (`SubagentStop`) can fire minutes after the parent session's `Stop` event, which would incorrectly overwrite `done` back to `executing`.

**Only these events may transition out of `done`:**
- `UserPromptSubmit` — user started a new task
- `SessionStart` / `SessionEnd` — session lifecycle
- `Stop` / `StopFailure` — new turn ending or failed turn

**All other events are ignored when status is `done`:**
- `SubagentStart`, `SubagentStop` — late background subagent completions
- `PreToolUse`, `PostToolUse` — stale tool events
- `Notification`, `PermissionRequest` — informational events

## Event -> Status Mapping (`mapEventToStatus`)

### Stop

| Condition                                                      | Result    |
| -------------------------------------------------------------- | --------- |
| `stop_hook_active: true`                                       | `waiting` |
| `currentStatus === "waiting"`                                  | `waiting` |
| `permission_mode === "plan"` or `currentStatus === "planning"` | `waiting` |
| `currentStatus === "executing"`                                | `done`    |
| Fallback                                                       | `idle`    |

### StopFailure

| Condition | Result |
| --------- | ------ |
| Always    | `idle` |

### Notification

| Condition                                                             | Result             |
| --------------------------------------------------------------------- | ------------------ |
| `notification_type === "permission_prompt"` or `"elicitation_dialog"` | `waiting`          |
| `notification_type === "idle_prompt"` and `currentStatus !== "done"`  | `waiting`          |
| `notification_type === "idle_prompt"` and `currentStatus === "done"`  | No change (`null`) |
| Other notification types                                              | No change (`null`) |

### PermissionRequest

| Condition | Result    |
| --------- | --------- |
| Always    | `waiting` |

### SessionStart / SessionEnd

| Condition | Result |
| --------- | ------ |
| Always    | `idle` |

### UserPromptSubmit

| Condition                    | Result      |
| ---------------------------- | ----------- |
| `permission_mode === "plan"` | `planning`  |
| Otherwise                    | `executing` |

### Tool-specific

| Tool / Condition                               | Result     |
| ---------------------------------------------- | ---------- |
| `AskUserQuestion`                              | `waiting`  |
| `ExitPlanMode`                                 | `waiting`  |
| `EnterPlanMode`                                | `planning` |
| `PreToolUse` with `permission_prompt === true` | `waiting`  |

### Plan mode passthrough

| Condition                                                | Result     |
| -------------------------------------------------------- | ---------- |
| `permission_mode === "plan"` (any remaining event)       | `planning` |
| Tool/subagent event while `currentStatus === "planning"` | `planning` |

### Tool / Subagent activity

| Condition                                                       | Result      |
| --------------------------------------------------------------- | ----------- |
| `PreToolUse` / `PostToolUse` / `SubagentStart` / `SubagentStop` | `executing` |

### Fallback

| Condition       | Result |
| --------------- | ------ |
| No rule matched | `idle` |

## Display-time Override (`getDisplayStatus`)

| Condition                                                   | Displayed As |
| ----------------------------------------------------------- | ------------ |
| `executing` or `planning` with no event for 5+ minutes      | `waiting`    |
| Everything else                                             | As stored    |

This is a display-time safety net only. The DB status is preserved and resumes correctly when events arrive. The 5-minute threshold catches sessions where Claude crashed without firing a `Stop` event.

## Subscribed Hook Events

| Event              | Purpose                                         |
| ------------------ | ----------------------------------------------- |
| `PreToolUse`       | Track tool execution start                      |
| `PostToolUse`      | Track tool execution end                        |
| `Stop`             | Turn completed                                  |
| `StopFailure`      | Turn failed (API error)                         |
| `Notification`     | Permission prompts, idle prompts                |
| `SessionStart`     | Session opened                                  |
| `SessionEnd`       | Session closed                                  |
| `UserPromptSubmit` | User started a new task                         |
| `SubagentStart`    | Subagent spawned                                |
| `SubagentStop`     | Subagent completed                              |
| `PermissionRequest`| Claude Code 2.x managed permission prompt       |
