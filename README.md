# Agent Monitor (`am`)

TUI dashboard for managing git worktrees and monitoring Claude Code agents.

## Features

- **Multi-repo dashboard** — view worktrees from all added repos at once, grouped by repo
- Create, delete, and manage git worktrees from a single dashboard
- Monitor Claude Code agent status across worktrees
- View GitHub PR status for each worktree
- Linear ticket integration
- Run startup scripts per worktree
- Configurable settings for GitHub, Linear, and more

## Install

### From GitHub Packages

Since this is a private package, you need to authenticate first:

```sh
npm login --registry=https://npm.pkg.github.com
```

Use your GitHub username and a [personal access token (classic)](https://github.com/settings/tokens) with `read:packages` scope as the password.

Then configure your `~/.npmrc`:

```
@roylet-gs:registry=https://npm.pkg.github.com
```

And install:

```sh
npm install -g @roylet-gs/agent-monitor
```

### From source

```sh
git clone https://github.com/roylet-gs/agent-monitor.git
cd agent-monitor
pnpm install
pnpm build
pnpm link --global
```

## Usage

```sh
am                              # Launch the TUI dashboard (default)
am --help                       # Show all commands
```

### Worktree Commands

```sh
am worktree list [--repo <path>] [--json]    # List worktrees with status/PR/Linear
am worktree create <branch> [opts]           # Create worktree (--base, --reuse, --json)
am worktree delete <target> [opts]           # Delete worktree (--force, --delete-branch, --delete-remote)
am worktree open <target>                    # Open in IDE
am worktree sync [--repo <path>]             # Sync git state with DB
am worktree info <target> [--json]           # Detailed worktree info
```

### Repo Commands

```sh
am repo list [--json]            # List tracked repos
am repo add <path>               # Add a repo
am repo remove <name-or-path>    # Remove a repo
```

### Settings Commands

```sh
am settings list [--json]    # Show all settings
am settings get <key>        # Get one setting
am settings set <key> <val>  # Set one setting
am settings reset            # Reset to defaults
```

### Hooks

```sh
am hooks install       # Install Claude hooks into ~/.claude/settings.json
am hooks uninstall     # Remove agent-monitor hooks
am hooks status        # Check if hooks are installed
```

### PR & Linear

```sh
am pr [<target>] [--json]       # Show PR info
am pr open [<target>]           # Open PR in browser
am linear [<target>] [--json]   # Show Linear ticket info
am linear open [<target>]       # Open ticket in browser
```

### Scripts & Diagnostics

```sh
am script edit [--repo <path>]     # Create/open startup script in IDE
am script show [--repo <path>]     # Print script contents
am script remove [--repo <path>]   # Remove startup script
am doctor [--json]                 # Check hooks, gh CLI, DB, etc.
```

### Short Aliases

```sh
am ls                    # → worktree list
am new <branch>          # → worktree create
am open <target>         # → worktree open
```

### Internal / Backward Compat

```sh
am status -w <path> [--set <s>]    # Get/set agent status
am hook-event -w <path> [-e <e>]   # Receive hook events (used by hooks)
am install-hooks                   # Alias for: hooks install
am uninstall-hooks                 # Alias for: hooks uninstall
```

All read commands support `--json` for machine-readable output. `<target>` resolves flexibly by path or branch name.

## Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `j` / `↓` | Move down |
| `k` / `↑` | Move up |
| `Enter` | Open in IDE |
| `n` | New worktree (picks repo first if multiple) |
| `d` | Delete selected worktree |
| `s` | Settings |
| `r` | Refresh / sync all repos |
| `g` | Open PR in browser |
| `l` | Open Linear ticket |
| `q` / `Esc Esc` | Quit |

## Configuration

Settings are accessible via the `s` key in the dashboard. Stored at `~/.agent-monitor/settings.json`.

Configurable sections: Worktree, Agent, GitHub, Linear, Repositories.

## Requirements

- Node.js >= 18
- git
- gh CLI (optional, for GitHub PR status)
- Claude Code (for agent monitoring)
