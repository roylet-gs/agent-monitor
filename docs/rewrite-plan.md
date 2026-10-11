# Rewrite plan: `am` in Rust

Status: **Phase 0 built, pending rollout** (2026-10-11).

Phase 0 progress:
- [x] Cargo workspace, `am-core` (paths, logger, migrations, status rules), `am-hook` binary
- [x] Subagent tracking by `agent_id` (`subagents` table, schema v2), mirrored into `active_subagents` for the TS TUI
- [x] TS hooks installer prefers the native binary; re-install upgrades in place
- [x] Benchmark: native **2.5–9 ms / 3.4 MB** vs TS `am hook-event` **249 ms mean (146–703 ms) / 102 MB** (hyperfine, copy of the real DB)
- [ ] Switch live hooks (`cargo install --path crates/am-hook` + `am hooks install` from this branch)
- [ ] Run with `AM_HOOK_CAPTURE=1` for a day and check the real payloads (subagent fields on Notification/PermissionRequest, SubagentStop multiplicity)
- [ ] Ship the binary in the npm package (per-platform optional deps) — folded into Phase 4 distribution work

## Why

1. **Performance.** `am hook-event` runs on every Claude Code hook event, which is at least twice per tool call. It measured 0.21–0.30 s per call because `src/cli.tsx` statically imports Ink, React and the whole `App` tree before commander dispatches. A native binary does the same work in a few milliseconds. The TUI's poll loop also spawns about 4 git processes per worktree plus blocking `lsof`/`ps` calls every cycle.
2. **Flicker.** The root `<Box height={stdout.rows}>` (`app.tsx`) makes Ink 5 emit `clearTerminal` on every frame. `PulsingDot` forces about 5 full clears per second while any agent is active. The fix is a cell-diffing renderer (ratatui) on the alt screen with synchronized output (DEC mode 2026).
3. **Look.** Move to a sharper, lazygit-style layout with semantic-token theming.

## Decisions

| Topic | Decision |
|---|---|
| Stack | Rust + ratatui + crossterm, tokio, rusqlite (bundled), `tracing` |
| Layout | lazygit-style bordered panes: `[1] Worktrees` (left) and `[2] Subagents` (right) |
| Detail panel | **Removed.** PR, CI, Linear and git data move into the worktree rows; one footer line shows session ID and full ticket title |
| Subagents pane | Follows the selected worktree by default; a key toggles a global feed across all worktrees. Shows description, type, background/depth, elapsed time, tool count and current tool + input, plus recently finished subagents |
| Cut | Daemon (never actually ran); managed chat/sessions (`am agent send`, ChatView, `claude -p` turns) |
| Kept | Niche extras (suggest-feature, audio, log pane, standalone sessions, startup scripts, setup wizard), running-process detection |
| Enter | Open IDE. For vscode/cursor, copy `claude --resume <id>` to the clipboard (as today) |
| `c` | Open a terminal running `claude --resume <latest>`, with a session picker if there are several |
| Settings | Same `~/.agent-monitor/settings.json` keys (old and new apps run side by side). Slim key/value editor in the TUI, a sort-order modal, `e` opens the file in `$EDITOR` |
| Distribution | npm wrapper (`@roylet-gs/agent-monitor`, per-platform binaries as optional deps) **and** a Homebrew tap, with binaries attached to GitHub Releases |
| Repo layout | Cargo workspace in this repo: `crates/am-core`, `crates/am-hook`, `crates/am-tui`. TS is removed at cut-over |
| Theming | Semantic tokens. The default theme uses ANSI-16 and follows the terminal palette; catppuccin, tokyonight and gruvbox ship built in. Nerd Font icons are opt-in |
| Testing | ratatui `TestBackend` + `insta` snapshots. Pure rules (sort/grouping, status mapping) are ported with their tests. Docker E2E keeps its mock-api and drives the Rust binary |
| Narrow terminals | Below about 100 columns, panes stack and Subagents collapses to a one-line summary (`2` focuses it) |

## What to keep from the TS design

- SQLite (WAL) is the source of truth; the Unix socket is only a "something changed" hint.
- `FetchResult`: "absent" and "failed" are different results, and only a successful fetch writes to `integration_cache`.
- Per-repo GitHub backoff, with a forced refresh that bypasses it.
- `AM_DATA_DIR` isolation.
- The DB schema is shared in place, so the TS and Rust apps can run side by side during migration.

## Bugs found in the debug logs (fixed by the rewrite)

| Bug | Cause | Fixed in |
|---|---|---|
| Terminal detection has never worked (1,983/1,983 failures) | `lsof` exits 1 when one of the shells it was asked about isn't running, and `execSync` discards valid stdout (`process.ts:57,385`) | Phase 1: native libproc scan |
| IDE detection always returns 0 | Matches `/Code.app/` (the real app is `Visual Studio Code.app`) and splits args on whitespace | Phase 1: native scan |
| `delegating` never visible; worktrees stuck on `executing` | A single `active_subagents` counter. SubagentStop fires more often than SubagentStart, and subagent tool events (which share the parent `session_id`) overwrite the status | **Phase 0** |
| Linear has no offline backoff (2,483 WARNs overnight); multi-line `gh` stderr in logs | Fan-out per branch with no circuit breaker; raw stderr interpolated into log lines | **Phase 1 (core done)**: `am-core::integrations` — batched GraphQL + per-source breaker, one-line error summaries |
| PRs with commit statuses show "Checks Running" forever; PRs with >100 checks can show the wrong status | `statusCheckRollup` StatusContext entries have `state`, not `status`; only the first page of checks was considered | **Phase 1 (core done)**: StatusContext normalisation + `checkRunCountsByState` |
| Log "rotation" keeps under 1 h of history and loses concurrent lines | Read whole file → rewrite last 1,000 lines | Phase 0/1: rename-based rotation |
| WAL at 4.1 MB vs 233 KB DB | No checkpointing / `journal_size_limit` | **Phase 0** |
| Hook writes `tool_input` snippets to disk | Debug payload logging | Phase 0: only logs non-sensitive fields |

## Phases

### Phase 0: native hook binary (ships alongside the TS TUI)

- Cargo workspace with `am-core` (paths, DB, logging, status rules) and `am-hook` (binary).
- `am-hook hook-event --worktree <path> --event <E> [--managed]`: same flags, so installed hook commands keep working. The `cat |` prefix is no longer needed.
- Subagent tracking replaces the counter:
  - New `subagents` table keyed by `(session_id, agent_id)` with `worktree_id`/`standalone_path`, `agent_type`, `description`, `status` (running/done), `started_at`, `ended_at`, `last_tool`, `last_tool_input`, `tool_count`, `transcript_path`.
  - SubagentStart inserts a row; SubagentStop marks it done (idempotent: repeated stops are harmless).
  - Tool events carrying an `agent_id` update that subagent's row and **do not** change the parent worktree's status.
  - `active_subagents` stays populated (derived count of running rows) so the TS TUI keeps working.
  - The parent is `delegating` while the main turn has stopped and at least one subagent is running.
  - Reconciliation against `~/.claude/projects/<cwd>/<session>/subagents/agent-<id>.{meta.json,jsonl}` fills in description/type and recovers from missed stops (Phase 1 TUI does the transcript-tail work; the hook stays cheap).
- `PRAGMA user_version` migrations, `busy_timeout`, `journal_size_limit`, opportunistic `wal_checkpoint(PASSIVE)`.
- Raw payload capture (opt-in via `AM_HOOK_CAPTURE=1`) to confirm field names against real Claude Code payloads.
- Socket publish of the same `agent-status-update` / `git-activity` messages the TS TUI already consumes.
- Install path: `am hooks install` points at the native binary when present. Benchmark with `hyperfine`; the target is under 5 ms (currently about 250 ms).

### Phase 1: core library + read-only TUI

Native process scan (libproc). An integration scheduler with per-source circuit breakers and one GraphQL call per repo for PRs. Fewer git spawns; socket pings only re-read `agent_status`/`subagents`. ratatui with alt screen and synchronized output. Theming. `tracing` with rename-based rotation. Port `grouping`/sort registry/status mapping with their tests. Worktrees pane + Subagents pane (with transcript tailing for current tool and elapsed time).

### Phase 2: actions

New/delete/branch-exists modals, worktree limit, IDE and terminal launch, `claude --resume`, PR and Linear links, startup scripts, settings editor, sync, remaining CLI subcommands.

### Phase 3: polish and parity

Running processes, audio, log pane, standalone sessions, suggest-feature, setup wizard, `doctor`, narrow layout, Docker E2E pointed at the Rust binary.

### Phase 4: cut-over

Remove TS, publish via npm wrapper + Homebrew tap + GitHub Releases, rewrite CLAUDE.md and README.

## Open questions

- `~/.agent-monitor` contains `AgentMonitor.app`, `rules.json`, `roles`, `am-managed-permissions.json`, and settings has `showIdeStatus`. None are referenced on this branch. They are assumed to come from another branch or experiment and are ignored by the rewrite unless told otherwise.
