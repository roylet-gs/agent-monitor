# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Dev Commands

- **Package manager:** pnpm (v9)
- `pnpm build` — TypeScript compile (`tsc`) to `dist/`
- `pnpm start` — Run directly via `tsx src/cli.tsx` (no build needed)
- `pnpm dev` — Watch mode via `tsx --watch src/cli.tsx`
- No test framework or linter is configured

## What This Is

`agent-monitor` (`am`) is a terminal UI (TUI) dashboard for monitoring multiple Claude Code agent sessions across git worktrees. Built with Ink (React for CLIs) and SQLite.

Key capabilities: live agent status tracking, GitHub PR/CI status, Linear ticket integration, worktree lifecycle management (create/delete), IDE launching, per-repo startup scripts.

## Architecture

### Entry Flow
`src/cli.tsx` parses args with meow and routes to either a subcommand (`hook-event`, `status`) or launches the TUI. The TUI runs in a `while(true)` loop: Ink renders → user exits → unmount → run any pending startup script in raw terminal → re-launch Ink. This loop exists because Ink takes over stdin/stdout and can't coexist with interactive child processes.

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

### Persistence
All data at `~/.agent-monitor/`: SQLite DB (`agent-monitor.db`), `settings.json`, `debug.log` (auto-rotated), `scripts/<repo-id>.sh`.

## Documentation

When making changes to architecture, data flow, modules, or conventions, update this file (CLAUDE.md) and README.md to keep them in sync with the codebase.

## Conventions

- ESM (`"type": "module"`) with NodeNext module resolution
- TypeScript strict mode, all shared types in `src/lib/types.ts`
- React 18 JSX transform (`react-jsx`)
- Components are in `src/components/`, hooks in `src/hooks/`, core logic in `src/lib/`
- SQLite chosen over JSON files for concurrency safety (hooks write while TUI reads)
- Published to GitHub Packages as `@roylet-gs/agent-monitor`
