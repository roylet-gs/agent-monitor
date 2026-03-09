<h1 align="center">Agent Monitor</h1>

<p align="center">
  <strong>A terminal dashboard for managing git worktrees and monitoring Claude Code agents</strong>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/node-%3E%3D18-brightgreen?style=flat-square" alt="Node >= 18">
  <img src="https://img.shields.io/badge/platform-macOS%20%7C%20Linux-lightgrey?style=flat-square" alt="Platform">
</p>

<p align="center">
  Agent Monitor (<code>am</code>) gives you a single TUI dashboard to manage git worktrees across multiple repositories, track Claude Code agent activity in real time, and surface GitHub PR and Linear ticket status — all without leaving the terminal.
</p>

---

## Dashboard Preview

```
  Worktrees                            Detail
  ── acme-app ──────────────────────   Claude  ● Executing
  ▸ ● feature/auth     PR #42 ✓       Task: Implementing OAuth flow...
    ◌ bugfix/login      (draft)
  ── payments-api ─────────────────    PR #42  Draft
    ● feature/billing   LIN-521        ✓ Checks passing
    ─ main
                                       Linear  LIN-521
                                       In Progress · High priority

  [Enter] Open  [n]ew  [d]elete  [s]ettings  [q]uit
```

---

## Table of Contents

- [Quick Start](#quick-start)
- [Requirements](#requirements)
- [Installation](#installation)
- [Features](#features)
    - [Dashboard](#dashboard)
    - [Worktree Management](#worktree-management)
    - [Agent Monitoring](#agent-monitoring)
    - [GitHub Integration](#github-integration)
    - [Linear Integration](#linear-integration)
    - [Startup Scripts](#startup-scripts)
    - [Settings](#settings)
- [Keyboard Shortcuts](#keyboard-shortcuts)
- [CLI Reference](#cli-reference)
- [Configuration](#configuration)
- [How It Works](#how-it-works)
- [Troubleshooting](#troubleshooting)

---

## Quick Start

```sh
# 1. Authenticate with GitHub Packages (one-time)
# Create a PAT at https://github.com/settings/tokens with read:packages scope
echo "//npm.pkg.github.com/:_authToken=YOUR_GITHUB_PAT" >> ~/.npmrc
echo "@roylet-gs:registry=https://npm.pkg.github.com" >> ~/.npmrc

# 2. Install globally
npm install -g @roylet-gs/agent-monitor

# 3. Launch
am
```

The setup wizard walks you through everything on first launch — IDE preference, repositories, Claude Code hooks, and optional integrations.

---

## Requirements

- **Node.js** >= 18
- **git** — for worktree operations
- **gh CLI** — optional, for GitHub PR status ([install](https://cli.github.com))
- **Claude Code** — for agent monitoring features
- **Linear** — optional, for ticket tracking and GitHub PR status

---

## Installation

### From GitHub Packages

GitHub Packages requires authentication even for public packages. You'll need a free GitHub account and a [Personal Access Token (classic)](https://github.com/settings/tokens) with the `read:packages` scope.

Configure npm (one-time):

```sh
# Authenticate and point npm to GitHub Packages for the @roylet-gs scope
echo "//npm.pkg.github.com/:_authToken=YOUR_GITHUB_PAT" >> ~/.npmrc
echo "@roylet-gs:registry=https://npm.pkg.github.com" >> ~/.npmrc
```

Then install globally:

```sh
npm install -g @roylet-gs/agent-monitor
```

### From Source

```sh
git clone https://github.com/roylet-gs/agent-monitor.git
cd agent-monitor
pnpm install
pnpm build
pnpm link --global
```

### First Run

On first launch, the setup wizard configures:

- **IDE preference** — Cursor, VS Code, or Terminal
- **Repositories** — which repos to track
- **Claude Code hooks** — for real-time agent status monitoring
- **GitHub PR integration** — inline PR and CI status (requires `gh` CLI)
- **Linear integration** — automatic ticket linking (optional)

You can re-run the wizard anytime with `am setup`, or verify your setup with `am doctor`.

---

## Features

### Dashboard

The main view is a two-pane layout: worktrees grouped by repository on the left, with a detail panel on the right showing agent status, PR info, and Linear ticket data for the selected worktree.

```
  Worktrees                            Detail
  ── my-project ────────────────────   Claude  ● Executing
  ▸ ● feature/auth     PR #42 ✓       Task: Adding login endpoint...
    ◌ bugfix/crash      (draft)
    ─ main                             PR #42  Open
                                       ✓ CI passing · 2 approvals

  [Enter] Open  [n]ew  [d]elete  [s]ettings  [q]uit
```

The dashboard auto-refreshes: agent status updates are pushed instantly via a Unix domain socket, while GitHub and Linear data polls on a configurable interval.

### Worktree Management

Create, delete, and open git worktrees from the TUI or CLI. The new worktree form lets you set a branch name, base branch, and optional custom name:

```
  New Worktree
  Branch name: feature/payment-system
  Name (optional):
  Base branch: main
  [Tab] Next  [Enter] Create  [Esc] Cancel
```

Key commands:

- `am new <branch>` — create a worktree
- `am ls` — list all worktrees
- `am open <target>` — open in your configured IDE

### Agent Monitoring

Track Claude Code agent status across all your worktrees in real time. Status indicators:

| Status    | Indicator  | Meaning                                 |
| --------- | ---------- | --------------------------------------- |
| Executing | `●` green  | Agent is running tools or writing code  |
| Planning  | `●` cyan   | Agent is thinking / planning next steps |
| Waiting   | `●` yellow | Agent is waiting for user input         |
| Done      | `●` blue   | Agent finished its task                 |
| Idle      | `◌` gray   | No active agent session                 |

> [!IMPORTANT]
> Hooks must be installed for real-time agent monitoring. Run `am hooks install` or let the setup wizard handle it. Hooks write to `~/.claude/settings.json` and fire events that Agent Monitor captures.

### GitHub Integration

PR status appears inline next to each worktree — `PR #42 ✓`, `(draft)`, or CI status. The detail panel shows the full picture: title, review state, and check results.

- `am pr` — show PR info for the current branch
- `am pr open` — open the PR in your browser
- Press `g` in the dashboard to open the selected worktree's PR

Requires the `gh` CLI to be installed and authenticated.

### Linear Integration

Branch names containing Linear ticket identifiers (e.g., `feature/LIN-521-auth-flow`) are automatically linked to their tickets. The dashboard shows ticket status and priority in the detail panel.

- `am linear` — show ticket info for the current branch
- `am linear open` — open the ticket in your browser
- Press `l` in the dashboard to open the selected ticket

> [!NOTE]
> Linear integration requires a read-only API key. You can set it up through the setup wizard or via `am settings set linearApiKey <key>`. Generate one at [Linear Settings > Security & Access](https://linear.app/settings/account/security).

### Startup Scripts

Define per-repository shell scripts that run automatically after creating a new worktree. Useful for installing dependencies, setting up environment files, or running migrations.

- `am script edit` — create or open the startup script in your IDE
- `am script show` — print script contents
- `am script remove` — delete the startup script

Scripts are stored at `~/.agent-monitor/scripts/<repo-id>.sh`.

### Settings

All settings are accessible from the TUI (press `s`) or CLI. Categories include IDE preference, worktree defaults, GitHub, Linear, and polling intervals.

Settings are stored in `~/.agent-monitor/settings.json` (or `$AM_DATA_DIR/settings.json`) and can be edited directly. The TUI also provides an **Open settings.json** shortcut at the top of the settings panel to open the file in your configured editor.

```
  Settings
  ▸ Open settings.json              (Enter to open)
  ── Worktree ──────────────────────
    IDE                  cursor
    Branch prefix        feature/
    Base branch          main
    Hide main branch     yes
  ── GitHub ────────────────────────
    PR status            enabled
    Poll interval        3m
  ── Linear ────────────────────────
    Enabled              yes
    Poll interval        3m
```

CLI access: `am settings list`, `am settings get <key>`, `am settings set <key> <value>`.

<details>
<summary><strong>Example settings.json</strong></summary>

```json
{
  "ide": "cursor",
  "defaultBranchPrefix": "feature/",
  "defaultBaseBranch": "main",
  "pollingIntervalMs": 30000,
  "autoSyncOnStartup": true,
  "compactView": false,
  "hideMainBranch": true,
  "ghPrStatus": true,
  "ghPollingIntervalMs": 180000,
  "logLevel": "info",
  "linearEnabled": false,
  "linearApiKey": "",
  "linearPollingIntervalMs": 180000
}
```

</details>

---

## Keyboard Shortcuts

| Key             | Action                   |
| --------------- | ------------------------ |
| `j` / `↓`       | Move down                |
| `k` / `↑`       | Move up                  |
| `Enter`         | Open in IDE              |
| `n`             | New worktree             |
| `d`             | Delete selected worktree |
| `s`             | Settings                 |
| `r`             | Refresh / sync all repos |
| `g`             | Open PR in browser       |
| `l`             | Open Linear ticket       |
| `q` / `Esc Esc` | Quit                     |

---

## CLI Reference

All read commands support `--json` for machine-readable output. `<target>` resolves flexibly by path or branch name.

<details>
<summary><strong>Worktree</strong> — <code>am worktree</code> (alias: <code>wt</code>)</summary>

| Command                       | Description                                     | Flags                                                     |
| ----------------------------- | ----------------------------------------------- | --------------------------------------------------------- |
| `am worktree list`            | List worktrees with status, PR, and Linear info | `--repo <path>`, `--json`                                 |
| `am worktree create <branch>` | Create a new worktree                           | `--repo`, `--name`, `--base`, `--reuse`, `--json`         |
| `am worktree delete <target>` | Delete a worktree                               | `--repo`, `--force`, `--delete-branch`, `--delete-remote` |
| `am worktree open <target>`   | Open in IDE                                     | `--repo`                                                  |
| `am worktree sync`            | Sync git state with database                    | `--repo`                                                  |
| `am worktree info <target>`   | Show detailed worktree info                     | `--repo`, `--json`                                        |

**Short aliases:** `am ls` (list), `am new <branch>` (create), `am open <target>` (open)

</details>

<details>
<summary><strong>Repo</strong> — <code>am repo</code></summary>

| Command                         | Description                 | Flags    |
| ------------------------------- | --------------------------- | -------- |
| `am repo list`                  | List tracked repositories   | `--json` |
| `am repo add <path>`            | Add a repository to track   | `--json` |
| `am repo remove <name-or-path>` | Remove a tracked repository | —        |

</details>

<details>
<summary><strong>Settings</strong> — <code>am settings</code></summary>

| Command                         | Description                    | Flags    |
| ------------------------------- | ------------------------------ | -------- |
| `am settings list`              | Show all settings              | `--json` |
| `am settings get <key>`         | Get a setting value            | `--json` |
| `am settings set <key> <value>` | Set a setting value            | —        |
| `am settings reset`             | Reset all settings to defaults | —        |

</details>

<details>
<summary><strong>Hooks</strong> — <code>am hooks</code></summary>

| Command              | Description                                         | Flags    |
| -------------------- | --------------------------------------------------- | -------- |
| `am hooks install`   | Install Claude hooks into `~/.claude/settings.json` | —        |
| `am hooks uninstall` | Remove agent-monitor hooks                          | —        |
| `am hooks status`    | Check if hooks are installed                        | `--json` |

</details>

<details>
<summary><strong>PR</strong> — <code>am pr</code></summary>

| Command               | Description                        | Flags              |
| --------------------- | ---------------------------------- | ------------------ |
| `am pr [target]`      | Show PR info for a worktree branch | `--repo`, `--json` |
| `am pr open [target]` | Open PR in browser                 | `--repo`           |

</details>

<details>
<summary><strong>Linear</strong> — <code>am linear</code></summary>

| Command                   | Description             | Flags              |
| ------------------------- | ----------------------- | ------------------ |
| `am linear [target]`      | Show Linear ticket info | `--repo`, `--json` |
| `am linear open [target]` | Open ticket in browser  | `--repo`           |

</details>

<details>
<summary><strong>Script</strong> — <code>am script</code></summary>

| Command            | Description                       | Flags              |
| ------------------ | --------------------------------- | ------------------ |
| `am script edit`   | Create/open startup script in IDE | `--repo`           |
| `am script show`   | Print script contents             | `--repo`, `--json` |
| `am script remove` | Remove startup script             | `--repo`           |

</details>

<details>
<summary><strong>Doctor & Logs</strong></summary>

| Command     | Description                                     | Flags                                                         |
| ----------- | ----------------------------------------------- | ------------------------------------------------------------- |
| `am doctor` | Check system health (hooks, gh CLI, DB, Linear) | `--json`                                                      |
| `am logs`   | Show recent debug logs                          | `-n <lines>`, `-f` (follow), `--level`, `--module`, `--clear` |
| `am setup`  | Re-run the setup wizard                         | —                                                             |

</details>

---

## Configuration

All data is stored in `~/.agent-monitor/`:

| File                   | Purpose                                          |
| ---------------------- | ------------------------------------------------ |
| `settings.json`        | User preferences                                 |
| `agent-monitor.db`     | SQLite database (worktrees, repos, agent status) |
| `scripts/<repo-id>.sh` | Per-repo startup scripts                         |
| `debug.log`            | Debug log (auto-rotated)                         |
| `am.sock`              | Unix domain socket for real-time updates         |

<details>
<summary><strong>All settings</strong></summary>

| Key                       | Default      | Description                                               |
| ------------------------- | ------------ | --------------------------------------------------------- |
| `ide`                     | `"cursor"`   | IDE to open worktrees in (`cursor`, `vscode`, `terminal`) |
| `defaultBranchPrefix`     | `"feature/"` | Prefix for new branch names                               |
| `defaultBaseBranch`       | `"main"`     | Default base branch for new worktrees                     |
| `pollingIntervalMs`       | `30000`      | Dashboard polling interval (ms)                           |
| `autoSyncOnStartup`       | `true`       | Sync git state when dashboard launches                    |
| `compactView`             | `false`      | Use compact worktree list layout                          |
| `hideMainBranch`          | `true`       | Hide main/master branch from worktree list                |
| `ghPrStatus`              | `true`       | Enable GitHub PR status                                   |
| `ghPollingIntervalMs`     | `180000`     | GitHub PR polling interval (ms)                           |
| `ghRefreshOnManual`       | `true`       | Refresh GitHub data on manual refresh                     |
| `logLevel`                | `"info"`     | Log level (`debug`, `info`, `warn`, `error`)              |
| `linearEnabled`           | `false`      | Enable Linear integration                                 |
| `linearApiKey`            | `""`         | Linear API key (read-only)                                |
| `linearPollingIntervalMs` | `180000`     | Linear polling interval (ms)                              |
| `linearUseDesktopApp`     | `false`      | Open Linear links in desktop app                          |
| `linearRefreshOnManual`   | `true`       | Refresh Linear data on manual refresh                     |
| `linearAutoNickname`      | `true`       | Auto-set worktree nicknames from Linear ticket titles     |
| `maxLogSizeMb`            | `2`          | Max debug log file size in MB before rotation             |
| `applyGlobalRulesEnabled` | `false`      | Write auto-approval rules to `~/.claude/settings.json`    |
| `safeCommandsPresetEnabled` | `false`    | Auto-approve harmless read-only Bash commands             |

</details>

---

## How It Works

```
Claude Code hooks → am hook-event → SQLite → Unix socket → TUI renders
```

1. **Claude Code fires hook events** when agent status changes (tool use, planning, idle)
2. **`am hook-event`** receives the event via stdin, writes it to SQLite, and publishes to the Unix domain socket
3. **The TUI** listens on the socket for instant updates, with SQLite polling as a fallback
4. **GitHub and Linear** data is polled on separate intervals and cached in the database

Only one TUI instance owns the socket at a time — others fall back to polling. SQLite (WAL mode) is the source of truth, ensuring safe concurrent reads/writes between the TUI and hook events.

See [CLAUDE.md](./CLAUDE.md) for full architecture details.

---

## Troubleshooting

Run `am doctor` to check the health of your setup — it verifies hooks, `gh` CLI, database, and Linear connectivity.

Use `am logs` to view debug logs, with filtering by level or module:

```sh
am logs --level error           # Show only errors
am logs -f                      # Follow log output
am logs --module hooks          # Filter by module
```

<details>
<summary><strong>Common issues</strong></summary>

**Agent status not updating**

- Run `am hooks status` to verify hooks are installed
- Check that the worktree path in hook config matches your project directory
- View logs with `am logs --level debug --module hook` for detailed event tracking

**GitHub PR status not showing**

- Ensure `gh` CLI is installed and authenticated: `gh auth status`
- Check that `ghPrStatus` is enabled: `am settings get ghPrStatus`

**Linear tickets not linking**

- Verify the API key is set: `am settings get linearApiKey`
- Ensure `linearEnabled` is `true`: `am settings set linearEnabled true`
- Branch names must contain a Linear identifier (e.g., `LIN-521`)

**Database issues**

- Run `am doctor` to check DB health
- The database auto-migrates on startup — if corrupted, delete `~/.agent-monitor/agent-monitor.db` and re-add your repos

</details>
