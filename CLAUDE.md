# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Dev Commands

- **Package manager:** pnpm (v9)
- `pnpm build` — TypeScript compile (`tsc`) to `dist/`
- `pnpm start` — Run directly via `tsx src/cli.tsx` (no build needed)
- `pnpm dev` — Watch mode via `tsx --watch src/cli.tsx`
- `pnpm test` — Run vitest test suite (`tests/` directory)
- `pnpm test:e2e` — Run Docker-based E2E tests (builds containers, runs Playwright specs, exits)
- `pnpm test:e2e:build` — Pre-build Docker images without running tests
- `pnpm test:e2e:clean` — Tear down containers, remove images/volumes

## What This Is

`agent-monitor` (`am`) is a terminal UI (TUI) dashboard for monitoring multiple Claude Code agent sessions across git worktrees. Built with Ink (React for CLIs) and SQLite.

Key capabilities: live agent status tracking, GitHub PR/CI status, Linear ticket integration, worktree lifecycle management (create/delete), IDE launching, per-repo startup scripts, and managed Claude sessions (am starts and drives one headless Claude Code session per worktree).

## Architecture

### Entry Flow
`src/cli.tsx` uses `commander` for CLI parsing and routes to subcommands or launches the TUI (default action). Commands are organized as subcommand groups (`worktree`, `repo`, `settings`, `hooks`, `pr`, `linear`, `script`, `doctor`) with short aliases (`ls`, `new`, `open`). Each command handler lives in `src/commands/` and uses dynamic imports to avoid loading unnecessary modules. The TUI runs in a `while(true)` loop: Ink renders → user exits → unmount → run any pending startup script in raw terminal → re-launch Ink. This loop exists because Ink takes over stdin/stdout and can't coexist with interactive child processes.

### State Machine
`src/app.tsx` is the root component managing an `AppMode` string union that controls rendering:
`dashboard` → `folder-browse` → `repo-select` → `new-worktree` → `branch-exists` → `creating-worktree` → `delete-confirm` → `deleting-worktree` → `settings` → `chat` (per-worktree Claude session view)

### Data Flow
1. **Agent status in:** Claude Code fires hook events → `am hook-event --worktree $CLAUDE_PROJECT_DIR` receives JSON on stdin → writes to SQLite via `src/commands/hook-event.ts` → publishes to Unix domain socket for instant TUI update. `mapEventToStatus` maps events to an `AgentStatusType` (`idle`/`executing`/`planning`/`waiting`/`delegating`/`done`). To handle Claude Code's background-subagents feature, `hook-event.ts` keeps a live `active_subagents` counter on `agent_status` (`+1` on `SubagentStart`, `−1` floored at 0 on `SubagentStop`, reset on session boundaries): when a `Stop`/`idle_prompt` fires while the counter is `> 0`, the worktree reports **`delegating`** (background agents still running) instead of `waiting`/`done`, and transitions to `done` once the last subagent stops.
2. **Pub/sub layer:** Unix domain socket at `~/.agent-monitor/am.sock` provides instant push updates. The TUI starts a server (`src/lib/pubsub-server.ts`, managed by `src/hooks/usePubSub.ts`), and `hook-event` publishes fire-and-forget messages (`src/lib/pubsub-client.ts`). Only one TUI can own the socket; others fall back to polling. SQLite remains the source of truth — pub/sub is an optimization to avoid polling delay. Messages are newline-delimited JSON, typed in `src/lib/pubsub-types.ts`.
3. **TUI polling:** `src/hooks/useWorktrees.ts` still polls SQLite + git status (2s default), GitHub PRs (180s default), and Linear tickets (180s default) as a fallback. Uses JSON fingerprinting to skip re-renders when data hasn't changed. Filtering, sorting, and group/flat-list construction are all centralized in `src/lib/grouping.ts` and shared with the daemon (`applyWorktreeFilters`, `makeComparator`/`SORT_REGISTRY`, `buildGroups`) — the hook and daemon only pass config and consume the result, so they can't diverge. Worktree ordering is user-configurable via the `worktreeSort` setting (an ordered list of `{ key, direction, enabled }` criteria — the comparator walks enabled criteria in order, first non-zero wins). `buildGroups` produces one group per repo; repo sections keep config order unless the `repo` sort criterion is enabled, in which case they are ordered by repo name. There is **no separate grouping setting** — clustering by ticket or project is achieved purely through the sort order. `WorktreeList` renders three kinds of render-only headers: per-repo headers (when >1 repo), ticket-cluster headers (`— ENG-123 —` for tickets with 2+ adjacent worktrees), and — when the top enabled sort criterion is `linearProject` (passed down as `groupByProject`) — Linear-project cluster headers (`═ Project ═`) so the project grouping produced by the sort is visible. The invariant is that `flatWorktrees` order always equals the visual row order — headers never consume a selection index. Any worktree field read as a new sort key must be added to the `fingerprint` in `useWorktrees.ts` **and** `useDaemon.ts` (that's why `agent_status.updated_at` is fingerprinted, for the `lastActivity` sort).
4. **Managed sessions (agent control):** `am agent send` / the TUI chat view spawn a **detached** `claude -p --output-format stream-json` process per turn (`src/lib/claude-session.ts`), first turn with `--session-id <uuid>`, later turns with `--resume <uuid>`. Stdout/stderr append to `~/.agent-monitor/sessions/<uuid>.jsonl`, which is the transcript store; SQLite (`managed_sessions`) tracks the session UUID, turn pid, and turn count. Because turns are detached, they survive the TUI exiting. Transcript rendering (`loadTranscript`) prefers Claude Code's own project file (`~/.claude/projects/<encoded-cwd>/<uuid>.jsonl`) — which also covers interactive attach turns and sessions started outside am — falling back to am's stream-json log; the chat view (`src/components/ChatView.tsx` + `src/hooks/useChatTranscript.ts`) polls it for live updates. Sessions started manually at a worktree are **adopted**: hooks record their `session_id` in `agent_status`, the chat view shows their history, and the first am prompt resumes them. The detail panel (`WorktreeDetail`) surfaces `agent_status.session_id` as a "Session" row (full UUID, copyable for `claude --resume`). `discoverWorktreeSessions` finds every session started at the worktree root **or any subdirectory** (candidate project dirs matched by encoded-name prefix, confirmed via the `cwd` field inside each transcript); with multiple sessions, `c` opens a `SessionPicker` (`chat-pick` mode) before the chat, and resumed turns spawn from the session's original start directory (claude resolves `--resume` within the current project). On terminals ≥ `DETAIL_PANEL_MIN_COLS` (100) the chat renders **embedded**: `app.tsx` passes `ChatView` as `Dashboard`'s `chatPane` prop, replacing `WorktreeDetail`, with the `ActionBar` in `chatMode`; narrower terminals get the full-screen `ChatView` (which then renders its own hint bar). Regular hooks fire for managed sessions too, so `agent_status` works unchanged. `claude --resume <uuid>` (via `am agent attach` or `openClaudeInTerminal`) hands the same conversation to an interactive terminal. When the `resumeLastSession` setting is on (default), the dashboard **Open** action (`Enter`) also resumes the worktree's most-recent session (`discoverWorktreeSessions(path)[0]`): in `terminal` IDE mode the opened terminal runs `claude --resume` directly; in `vscode`/`cursor` mode `copyResumeCommand` (`src/lib/ide-launcher.ts`) **copies** the `claude --resume <id>` command to the system clipboard (via `copyToClipboard` — `pbcopy`/`clip`/`xclip`, no npm dep, no trailing newline), `handleOpen` shows a transient centered "Copied to clipboard" popup (a bordered box driven by the `notice` state in `app.tsx`, auto-clearing after ~5s, telling the user they can disable it in Settings), then **pauses ~600ms** so the popup is visible before calling `openInIde` — which opens the editor and steals focus. The user pastes the command into the editor's own terminal. We deliberately avoid keystroke injection into the integrated terminal — it needs macOS Accessibility permission and fails silently (osascript error 1002) without it.

### Key Modules
- `src/lib/db.ts` — SQLite with WAL mode. Tables: `repositories`, `worktrees`, `agent_status`, `standalone_sessions`, `managed_sessions`. Handles schema migrations.
- `src/lib/claude-session.ts` — managed Claude sessions: spawns detached headless `claude` turns, parses stream-json transcripts
- `src/lib/git.ts` — Git ops via `simple-git`. Worktree creation uses raw `git worktree` commands for `--force`/`-b` flag control.
- `src/lib/github.ts` — Shells out to `gh` CLI for PR info
- `src/lib/linear.ts` — Raw HTTPS POST to Linear GraphQL API (no SDK). Fetches ticket info incl. the issue's project (`LinearInfo.project`).
- `src/lib/grouping.ts` — Shared filter/sort/group core used by both `useWorktrees` and the daemon: `applyWorktreeFilters` (which worktrees show), `SORT_REGISTRY` + `makeComparator` (the customizable `worktreeSort` comparator), and `buildGroups` (one group per repo, worktrees sorted by the criteria). Display-field toggles (`showPrStatus`, `showLinearTicket`, `showGitAheadBehind`, `showLastCommit`) are threaded from settings through `app.tsx` → `Dashboard` into `WorktreeList`/`WorktreeDetail`. The `worktreeSort` list is edited in a dedicated full-page editor (`src/components/SortEditor.tsx`) that `SettingsPanel` renders in place of itself while `sortEditing` is true: a grab-to-move criteria list (Enter grabs an item, ↑↓ moves it, Enter drops) alongside a live example-preview list re-sorted by `makeComparator` so the effect of each change is visible.
- `src/lib/hooks-installer.ts` — Writes Claude hook config to `.claude/settings.json` in worktrees
- `src/lib/settings.ts` — Loads/saves `~/.agent-monitor/settings.json`
- `src/lib/worktree-limit.ts` — `checkWorktreeLimit(settings, repoId, repoName)`: optional, off-by-default **per-repo** cap (`worktreeLimitEnabled` + `maxWorktrees`, default 5) on dedicated worktrees (excludes the main checkout via `is_main`). Returns a block message or null; **hard-blocks** creation in both the TUI (`app.tsx`: at the `onNew`/`handlePickRepoForCreate` entry points, before the form opens, plus a backstop in `doCreateWorktree`) and the CLI (`worktree/create.ts`, after `resolveRepo`). Shared so both enforce identically.
- `src/lib/output.ts` — CLI output formatting (table, key-value, JSON) for non-TUI commands
- `src/lib/resolve.ts` — Resolves CLI targets (branch names, paths) to DB entities; CWD-based repo detection

### CLI Commands
Commands in `src/commands/` are organized by domain:
- `src/commands/worktree/` — list, create, delete, open, sync, info
- `src/commands/agent/` — send, list, log, attach, stop (managed Claude sessions)
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
All data at `~/.agent-monitor/` (or `$AM_DATA_DIR` if set): SQLite DB (`agent-monitor.db`), `settings.json`, `debug.log` (auto-rotated), `scripts/<repo-id>.sh`, `sessions/<session-id>.jsonl` (managed session transcripts).

### Data Isolation (`AM_DATA_DIR`)
`src/lib/paths.ts` exports `APP_DIR` which defaults to `~/.agent-monitor/` but can be overridden via the `AM_DATA_DIR` environment variable. All other paths (`DB_PATH`, `SETTINGS_PATH`, `LOG_PATH`, `SOCKET_PATH`, etc.) derive from `APP_DIR` automatically. This enables running the app against isolated temp directories for testing without touching real user data.

## Testing

### Unit Tests (vitest)
Unit and component tests live in `tests/` mirroring the `src/` structure. Run with `pnpm test`. Uses `ink-testing-library` for React component tests. All tests use isolated `AM_DATA_DIR` temp directories.

### Docker E2E Tests
End-to-end tests run the real TUI in Docker containers with mocked external services. This is the primary way to capture visual evidence (screenshots) and test full user flows.

**Architecture:** Three Docker containers orchestrated by `e2e/docker-compose.yml`:
1. **`mock-api`** — HTTP server (`e2e/mock-api/server.ts`) that serves configurable fixtures for `gh` CLI and Linear API responses. Fixtures live in `e2e/mock-api/fixtures/`.
2. **`app`** — The real TUI served via ttyd on port 7681. Uses a fake `gh` shim (`e2e/bin/gh`) that proxies to mock-api. Entrypoint (`e2e/entrypoint.sh`) seeds a git repo and skips the setup wizard.
3. **`tests`** — Playwright container that connects to the TUI via browser, interacts with it, and captures screenshots.

**Test specs** live in `tests/e2e/docker/` and use two helpers:
- `helpers/tui-page.ts` — Page Object Model for the TUI. Methods: `goto()`, `getScreenText()` (reads xterm.js terminal buffer), `waitForText(text)`, `sendKey(key)`, `type(text)`, `screenshot(name)`.
- `helpers/mock-api-client.ts` — `setupMock({gh?, linear?})` to inject per-test fixtures, `resetMock()` in `beforeEach` for isolation.

**Existing specs and coverage:**

| Spec | What it tests |
|------|---------------|
| `dashboard.spec.ts` | Dashboard renders with seeded repo, shows repo name |
| `navigation.spec.ts` | Keyboard nav: `s`→settings, `n`→new worktree, `d`→delete |
| `pr-status.spec.ts` | Open PR, draft PR, and no-PR states (uses mock fixtures) |
| `linear-grouping.spec.ts` | Linear project header renders above grouped worktrees; selection traverses rows only |
| `setup-wizard.spec.ts` | Verifies wizard is skipped when settings are seeded |
| `screenshots.spec.ts` | Baseline screenshots: dashboard, settings, dashboard with PR |

**Writing new E2E specs:**
```typescript
import { test } from "@playwright/test";
import { TuiPage } from "./helpers/tui-page";
import { resetMock, setupMock } from "./helpers/mock-api-client";

test.beforeEach(async () => { await resetMock(); });

test("my feature works", async ({ page }) => {
  await setupMock({ gh: { number: 42, state: "OPEN" } }); // optional
  const tui = new TuiPage(page);
  await tui.goto();
  await tui.waitForText("main", 10_000);
  await tui.sendKey("s"); // navigate to settings
  await tui.screenshot("my-feature");
});
```

**Adding mock fixtures:** Add JSON files to `e2e/mock-api/fixtures/`, then use `setupMock()` to select them or pass inline objects. The mock-api routes are:
- `POST /gh` — handles gh CLI calls (args-based routing: `pr view`, `--version`)
- `POST /linear` — handles Linear GraphQL queries (viewer, issue search)
- `POST /mock/setup` — per-test fixture injection
- `DELETE /mock/reset` — restore defaults

**Running E2E tests:**
```bash
pnpm test:e2e          # Build + run + exit with test code
pnpm test:e2e:clean    # Tear down containers after
```
Screenshots land in `tests/e2e/tmp/`. New specs are automatically picked up via Dockerfile `COPY` on rebuild.

**Config:** `e2e/playwright.config.ts` — single Chromium worker, 30s timeout, 1 retry, screenshots on failure. Tests run sequentially (`workers: 1`) because they share a single TUI terminal.

## Evidence Capture & PR Workflow

### Slash Commands
- `/capture-evidence` — Spawns two parallel sub-agents: Agent A runs `pnpm test` for unit tests, Agent B runs `pnpm test:e2e` for Docker E2E tests and screenshots. Includes a coverage mapping table so Agent B knows which specs cover which components and can create new specs for gaps.
- `/create-pr` — Idempotent PR creation: detects existing PRs via `gh pr view`, runs `/capture-evidence`, then creates or updates the PR with evidence.

### Scripts (`.claude/scripts/`)
- `upload-evidence.sh [evidence-dir]` — Uploads screenshots as assets to the `evidence-store` GitHub release via `gh release upload --clobber`. Outputs release asset URLs. Called with `tests/e2e/tmp` as the evidence dir.
- `seed-evidence-data.sh` — **Deprecated.** For local manual debugging only. Use `pnpm test:e2e` instead.
- `capture-tui.sh` — **Deprecated.** For local manual debugging only. Use `pnpm test:e2e` instead.
- `cleanup-tui.sh` — **Deprecated.** For local manual debugging only. Use `pnpm test:e2e:clean` instead.

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
