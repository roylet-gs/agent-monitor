# Agent Status Mapping

## Status Types

| Status      | Meaning                                                                                                                                                                                                    |
| ----------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `none`      | Session not running, no clude session linked                                                                                                                                                               |
| `idle`      | session ended / session is not asking any questions or waiting on any active user input                                                                                                                    |
| `executing` | Claude is actively running tools or generating                                                                                                                                                             |
| `planning`  | Claude is in plan mode                                                                                                                                                                                     |
| `waiting`   | Claude is waiting for user input and has not completed it's task                                                                                                                                           |
| `done`      | Claude finished a task and requires no more use input (this is slightly diffrent from idle as done means claude did some work and is now done while idle means claude has done nothing yet and is in idle) |

## Event → Status Mapping (`mapEventToStatus`)

### Stop

| Condition                                                      | Result    |
| -------------------------------------------------------------- | --------- |
| `stop_hook_active: true`                                       | `waiting` |
| `currentStatus === "waiting"`                                  | `waiting` |
| `permission_mode === "plan"` or `currentStatus === "planning"` | `waiting` |
| `currentStatus === "executing"`                                | `done`    |
| Fallback                                                       | `idle`    |

### Notification

| Condition                                                             | Result             |
| --------------------------------------------------------------------- | ------------------ |
| `notification_type === "permission_prompt"` or `"elicitation_dialog"` or `"idle_prompt"` | `waiting`          |
| Other notification types                                              | No change (`null`) |

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
| `EnterPlanMode` + `PreToolUse`                 | `waiting`  |
| `EnterPlanMode` + `PostToolUse`                | `planning` |
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

| Condition       | Displayed As |
| --------------- | ------------ |
| Everything      | As stored    |
