# Agent Monitor (`am`)

TUI dashboard for managing git worktrees and monitoring Claude Code agents.

## Features

- Create, delete, and manage git worktrees from a single dashboard
- Monitor Claude Code agent status across worktrees
- View GitHub PR status for each worktree
- Linear ticket integration
- Run startup scripts per worktree
- Configurable settings for GitHub, Linear, and more

## Install

### From source

```sh
git clone https://github.com/your-org/agent-monitor.git
cd agent-monitor
pnpm install
pnpm build
pnpm link --global
```

> Package registry publishing TBD

## Usage

```sh
am                            # Launch the dashboard
am status -w <path>           # Print agent status for a worktree
am install-hooks <path>       # Install Claude hooks into a worktree
am hook-event -w <path>       # Receive hook event from stdin (used by hooks)
```

## Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `j` / `↓` | Move down |
| `k` / `↑` | Move up |
| `Enter` | Open in IDE |
| `n` | New worktree |
| `d` | Delete worktree |
| `s` | Settings |
| `r` | Refresh / sync |
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
