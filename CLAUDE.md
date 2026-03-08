# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Dev Commands

- **Package manager:** pnpm (v9)
- `pnpm build` — TypeScript compile (`tsc`) to `dist/`
- `pnpm start` — Run directly via `tsx src/cli.tsx` (no build needed)
- `pnpm dev` — Watch mode via `tsx --watch src/cli.tsx`
- `pnpm test` — Run vitest test suite (`tests/` directory)

## What This Is

`agent-monitor` (`am`) is a terminal UI (TUI) dashboard for monitoring multiple Claude Code agent sessions across git worktrees. Built with Ink (React for CLIs) and SQLite.

Key capabilities: live agent status tracking, GitHub PR/CI status, Linear ticket integration, worktree lifecycle management (create/delete), IDE launching, per-repo startup scripts.

## Architecture

### Entry Flow
`src/cli.tsx` uses `commander` for CLI parsing and routes to subcommands or launches the TUI (default action). Commands are organized as subcommand groups (`worktree`, `repo`, `settings`, `hooks`, `pr`, `linear`, `script`, `doctor`) with short aliases (`ls`, `new`, `open`). Each command handler lives in `src/commands/` and uses dynamic imports to avoid loading unnecessary modules. The TUI runs in a `while(true)` loop: Ink renders → user exits → unmount → run any pending startup script in raw terminal → re-launch Ink. This loop exists because Ink takes over stdin/stdout and can't coexist with interactive child processes.

### State Machine
`src/app.tsx` is the root component managing an `AppMode` string union that controls rendering:
`dashboard` → `folder-browse` → `repo-select` → `new-worktree` → `branch-exists` → `creating-worktree` → `delete-confirm` → `deleting-worktree` → `settings`

### Data Flow
1. **Agent status in:** Claude Code fires hook events → `am hook-event --worktree $CLAUDE_PROJECT_DIR` receives JSON on stdin → writes to SQLite via `src/commands/hook-event.ts` → publishes to Unix domain socket for instant TUI update
2. **Pub/sub layer:** Unix domain socket at `~/.agent-monitor/am.sock` provides instant push updates. The TUI starts a server (`src/lib/pubsub-server.ts`, managed by `src/hooks/usePubSub.ts`), and `hook-event` publishes fire-and-forget messages (`src/lib/pubsub-client.ts`). Only one TUI can own the socket; others fall back to polling. SQLite remains the source of truth — pub/sub is an optimization to avoid polling delay. Messages are newline-delimited JSON, typed in `src/lib/pubsub-types.ts`.
3. **TUI polling:** `src/hooks/useWorktrees.ts` still polls SQLite + git status (2s default), GitHub PRs (60s), and Linear tickets (60s) as a fallback. Uses JSON fingerprinting to skip re-renders when data hasn't changed.

### Key Modules
- `src/lib/db.ts` — SQLite with WAL mode. Tables: `repositories`, `worktrees`, `agent_status`. Handles schema migrations.
- `src/lib/git.ts` — Git ops via `simple-git`. Worktree creation uses raw `git worktree` commands for `--force`/`-b` flag control.
- `src/lib/github.ts` — Shells out to `gh` CLI for PR info
- `src/lib/linear.ts` — Raw HTTPS POST to Linear GraphQL API (no SDK)
- `src/lib/hooks-installer.ts` — Writes Claude hook config to `.claude/settings.json` in worktrees
- `src/lib/settings.ts` — Loads/saves `~/.agent-monitor/settings.json`
- `src/lib/output.ts` — CLI output formatting (table, key-value, JSON) for non-TUI commands
- `src/lib/resolve.ts` — Resolves CLI targets (branch names, paths) to DB entities; CWD-based repo detection

### CLI Commands
Commands in `src/commands/` are organized by domain:
- `src/commands/worktree/` — list, create, delete, open, sync, info
- `src/commands/repo/` — list, add, remove
- `src/commands/settings/` — list, get, set, reset
- `src/commands/hooks.ts` — install, uninstall, status
- `src/commands/pr.ts` — show, open
- `src/commands/linear.ts` — show, open
- `src/commands/script.ts` — edit, remove, show
- `src/commands/doctor.ts` — system health check
- `src/commands/status.ts` — get/set agent status (unchanged)
- `src/commands/hook-event.ts` — receive hook events from stdin (unchanged)

### Persistence
All data at `~/.agent-monitor/` (or `$AM_DATA_DIR` if set): SQLite DB (`agent-monitor.db`), `settings.json`, `debug.log` (auto-rotated), `scripts/<repo-id>.sh`.

### Data Isolation (`AM_DATA_DIR`)
`src/lib/paths.ts` exports `APP_DIR` which defaults to `~/.agent-monitor/` but can be overridden via the `AM_DATA_DIR` environment variable. All other paths (`DB_PATH`, `SETTINGS_PATH`, `LOG_PATH`, `SOCKET_PATH`, etc.) derive from `APP_DIR` automatically. This enables running the app against isolated temp directories for testing without touching real user data.

## Evidence Capture & PR Workflow

### Slash Commands
- `/capture-evidence` — Orchestrates parallel sub-agents to collect build, test, and visual evidence. Uses diff analysis (`git diff main...HEAD`) to determine which tests and E2E scenarios are relevant.
- `/create-pr` — Idempotent PR creation: detects existing PRs via `gh pr view`, runs `/capture-evidence`, then creates or updates the PR with evidence.

### Scripts (`.claude/scripts/`)
- `seed-evidence-data.sh` — Creates an isolated `AM_DATA_DIR` in `/tmp/am-evidence-*` and seeds it with `repo add .`. Source it (don't execute) to get env vars.
- `capture-tui.sh <command> [screenshot-dir] [port]` — Starts ttyd serving the TUI. Propagates `AM_DATA_DIR` to the child process if set. Outputs the URL.
- `cleanup-tui.sh` — Stops ttyd and cleans up the evidence data dir (only removes `/tmp/am-evidence-*` paths for safety).
- `upload-evidence.sh [evidence-dir]` — Pushes screenshots from `.github/evidence/` to the `evidence-images` orphan branch. Outputs `raw.githubusercontent.com` URLs.

### Headless Browser
`.mcp.json` configures the Playwright MCP server with `--headless` so TUI screenshots are captured without a visible browser window.

### GitHub API Note
`gh pr edit --body` triggers a "Projects (classic) is being deprecated" GraphQL error. Use the REST API instead:
```bash
gh api "repos/OWNER/REPO/pulls/NUMBER" -X PATCH -f body='...'
```

## Documentation

When making changes to architecture, data flow, modules, or conventions, update this file (CLAUDE.md) and README.md to keep them in sync with the codebase.

## Logging

When adding or modifying code paths, add appropriate `log()` calls to maintain observability:
- `log("info", ...)` for key state changes (status updates, hook install/uninstall, config changes)
- `log("debug", ...)` for branching decisions, mapping logic, and detailed context (e.g. which branch was taken in `mapEventToStatus` and why)
- `log("warn", ...)` for recoverable errors or unexpected state
- Always include relevant context in log messages (event names, tool names, computed values) to aid debugging

## Conventions

- ESM (`"type": "module"`) with NodeNext module resolution
- TypeScript strict mode, all shared types in `src/lib/types.ts`
- React 18 JSX transform (`react-jsx`)
- Components are in `src/components/`, hooks in `src/hooks/`, core logic in `src/lib/`
- SQLite chosen over JSON files for concurrency safety (hooks write while TUI reads)
- Published to GitHub Packages as `@roylet-gs/agent-monitor`
