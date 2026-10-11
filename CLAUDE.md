# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Dev Commands

- **Toolchain:** stable Rust (edition 2024), Cargo workspace in `crates/`. pnpm (v9) is only used for the Docker E2E harness.
- `cargo build --workspace` — debug build of `am`, `am-tui`, `am-hook` into `target/debug/`
- `cargo run -p am-cli -- <args>` — run the CLI (`am`). With no args it execs `am-tui` (next to the exe, else `$PATH`), so for the dashboard use `cargo run -p am-tui`
- `cargo test --workspace` — unit, snapshot and integration tests
- `cargo clippy --workspace --all-targets -- -D warnings` — lint (CI fails on any warning)
- `INSTA_UPDATE=always cargo test -p am-tui` — accept changed insta snapshots (`crates/am-tui/src/snapshots/*.snap`); review the diff before committing
- `cargo install --path crates/am-cli && cargo install --path crates/am-tui && cargo install --path crates/am-hook` — install all three binaries to `~/.cargo/bin`, then `am hooks install` to point Claude Code at `am-hook`
- `cargo run -p am-core --example integrations_probe -- <repo> <branch>...` — manual GitHub/Linear fetch against a throwaway DB
- `pnpm test:e2e` — Docker E2E: build images, run the Playwright specs against the real binaries, exit with the tests' exit code
- `pnpm test:e2e:build` — pre-build the Docker images only
- `pnpm test:e2e:clean` — tear down containers, remove local images and volumes

## What This Is

`agent-monitor` (`am`) is a terminal dashboard for monitoring Claude Code agent sessions across git worktrees, written in Rust (ratatui + crossterm, rusqlite). Version 2.x is a native rewrite of the 1.x TypeScript/Ink app and reads and writes the same SQLite DB and `settings.json` in place (history and decisions: `docs/rewrite-plan.md`).

Key capabilities: live agent status from Claude Code hooks (including background subagents → `delegating`), GitHub PR/CI status, Linear tickets and project grouping, stacked-branch detection, worktree create/delete with a live "exists on origin" check, IDE/terminal launching with "resume last Claude session", per-repo startup scripts, dev-server detection, and a scriptable CLI with `--json` output.

## Architecture

Three binaries over one library:

| Crate | Binary | Role |
|---|---|---|
| `am-core` | (lib) | Paths, logger, DB + migrations, status rules, settings, grouping/sort, git/worktree ops, integrations, process scan, launching |
| `am-hook` | `am-hook` (lib + bin) | Claude Code hook handler, on the per-event hot path (~3 ms) |
| `am-tui` | `am-tui` | ratatui dashboard |
| `am-cli` | `am` | clap CLI; no subcommand execs `am-tui` |

### Entry Flow
`am` (`crates/am-cli/src/main.rs`) first checks `argv[1] == "hook-event"` and, if so, calls `am_hook::run_hook_event` **before clap, settings or logger init** (same code as `am-hook`, ~2 ms). Otherwise clap parses the command; usage errors exit 1 (help/version exit 0), matching 1.x scripts. No subcommand `exec`s `am-tui` via `commands::find_sibling_or_path`; `am setup` passes `--setup` (reopen the wizard) and `am --watch` passes `--log` (open the log pane). Command handlers live in `crates/am-cli/src/commands/`.

`am-tui` (`crates/am-tui/src/main.rs`) loads settings, spawns the collector, shows the first-run setup wizard when `settings.json` lacks `setupCompleted: true`, and runs a single-threaded event loop: drain snapshots and launcher messages → draw only when something changed (or the clock second ticked), inside synchronized-output escapes (DEC 2026) → poll input for 100 ms. Anything slow (git, gh, launching, create/delete) runs on a spawned thread and reports back over an `mpsc` channel (`launcher::Msg`). Interactive child processes (startup script, `$EDITOR` on `settings.json`) temporarily `ratatui::restore()` and then re-init the terminal.

### UI State
`app.rs` is pure state + key handling (no I/O): a `Snapshot` comes in, `on_key` returns an `Action` that `main.rs` executes. Overlays take keys in priority order: popup (any key closes) → setup wizard → modal (`modals.rs`: new worktree, branch exists, delete confirm, add repo, startup-script prompt, suggest feature, busy) → settings editor (`settings_ui.rs`) → help → dashboard keys.

`rebuild_rows` turns `snapshot.groups` into visual `Row`s: repo headers (when >1 group, or for the "Other sessions" group), `═ Project ═` headers when the top enabled sort key is `linearProject` (blank line between project sections), `— ENG-123 title —` headers for a ticket shared by 2+ adjacent top-level rows, and worktree rows. Stacked branches are placed depth-first under their parent with tree prefixes (`├─ `, `└─ `) by `stack_order`; headers only consider top-level rows, so a stack is never split. **Invariant:** headers never take a selection index; `flat` order always equals the visual order of `Row::Worktree` rows. Selection follows the worktree id across refreshes; `pending_select` selects a just-created worktree once it appears. Status changes on unselected rows mark them unseen (`•`) and queue audio cues (`afplay` system sounds) for `waiting`/`done` when `audioNotifications` is on.

`ui.rs` renders one full-width `─Worktrees` pane (bottom-right `N of M`), an optional log pane (`w`, tail of `debug.log`), and a one-line key bar whose hints drop by priority as the width shrinks, with a transient notice or the `⚠ <source> offline — cached` hint right-aligned. A row is: selection bar `▌`, tree prefix, status glyph, display name (nickname first, then the branch dimmed), badges (`•` unseen, `▶` running processes, `◆ N subagents`), then fixed-width columns: Linear ticket, PR (`#42` + `✓`/`✗`/`◌`, `⇡` when merged; coloured by state), git (`↑ahead ↓behind ±dirty`), last-commit age. Columns follow the `show*` settings. `theme.rs` holds semantic colour tokens for `ansi` (default, follows the terminal palette; selection is reverse video), `catppuccin-mocha`, `tokyonight` and `gruvbox`.

### Data Flow
1. **Agent status in:** Claude Code runs `"<abs path>/am-hook" hook-event --worktree "$CLAUDE_PROJECT_DIR" --event <E>` (written by `am hooks install`) with the JSON payload on stdin. `am_hook` walks up to the directory containing `.git` (no git spawn), finds the worktree row (otherwise tracks the path as a standalone session in `standalone_sessions`), does subagent bookkeeping, then applies `am_core::status::map_event_to_status` and upserts `agent_status`. Hooks never fail Claude Code: errors are logged and the process always exits 0. Full rules: `STATUS_MAPPING.md`.
2. **Subagents:** tracked per `(session_id, agent_id)` in the `subagents` table (schema v2), not with a counter. Tool events carrying `agent_id` come *from inside* a subagent: they update only that subagent's row (tool count, last tool + summarized input) and never move the parent status, except a permission prompt → `waiting`. Internal agents (empty `agent_type`) are ignored, duplicate `SubagentStop`s are idempotent, a running row only counts if it was updated in the last 2 h, `SessionStart` closes subagents from other sessions and `SessionEnd` closes the session's own. Descriptions come from Claude Code's `<session>/subagents/agent-<id>.meta.json`. The live count is mirrored into `agent_status.active_subagents` for 1.x compatibility. When the main turn stops with subagents still running the worktree is `delegating`; it becomes `done` when the last one stops.
3. **TUI collector (`am-tui/src/data.rs`):** a `collector` thread holds a long-lived connection and polls `PRAGMA data_version` every 200 ms. The value changes whenever another connection (a hook) commits, so hook writes appear within ~200 ms with no socket. Per cycle: `worktrees::sync_all` every 10 s (picks up worktrees made outside am); a native process scan every 2 s (`procscan`: terminals, VS Code/Cursor windows, dev servers); git status (`gitinfo`, 2 spawns per worktree, run concurrently) only for new rows and worktrees whose `agent_status.updated_at` moved, plus a full sweep every `pollingIntervalMs` (min 5 s) or on `r`; stack inference (`stacks`) when branch tips change. It then builds views → `grouping::apply_filters` → `grouping::build_groups` → appends "Other sessions" (effectively-open standalone sessions) and sends a `Snapshot` only if it differs from the last one. The UI coalesces to the newest snapshot.
4. **Integrations thread:** a separate `integrations` thread owns `am_core::integrations::Integrations`. It hydrates from the `integration_cache` table and publishes cached PR/Linear fields before the first fetch, then refreshes GitHub every `ghPollingIntervalMs` and Linear every `linearPollingIntervalMs` (min 30 s each), immediately when the set of branches changes, and on `r` (`force`, subject to `ghRefreshOnManual`/`linearRefreshOnManual`). After a refresh it applies Linear auto-nicknames (`linearAutoNickname`; user-set names are never touched) and sends fields + health to the collector.
5. **Integration cache (offline resilience):** `integrations/cache.rs` uses `integration_cache(kind, cache_key, payload, fetched_at)` with `kind` `pr`/`linear`, `cache_key` `<repo_id>:<branch>`, and camelCase JSON payloads (NULL = fetched OK, none exists). **Only a successful fetch writes**; a failure leaves the old value, so PR badges and ticket/project clustering survive going offline. No TTL: rows are replaced by a later success or pruned when their worktree is gone (an empty worktree list never prunes). `breaker.rs` is a per-source circuit breaker: a network-class failure (DNS, refused, "error connecting", timeouts, 5xx/429) aborts the rest of that source's cycle and backs off 30 s doubling to 15 min; other failures mark the source failing without backoff; WARN only on healthy→failing, INFO on recovery, DEBUG in between; `force` bypasses the backoff. `health()` drives the offline hint. When GitHub has no PR, the Linear ticket's PR attachment for that branch stands in (without checks).
6. **Stacked branches (`stacks.rs`):** a worktree's parent is the worktree whose branch its open PR targets (`baseRefName`). Failing that, git decides: the other worktree branch whose tip is among this branch's commits not yet on the trunk (`origin/<main>` when it exists), choosing the closest one (most unmerged commits). Rev-lists rerun only when a branch tip moves.
7. **Opening (`am-tui/src/launcher.rs` + `am_core::launch`):** `Enter` opens the worktree in the `ide` setting. With `resumeLastSession` on and no session already visibly open, it resumes the newest Claude session found under `~/.claude/projects/` for the worktree root or any subdirectory (`launch::discover_sessions`, confirmed via each transcript's `cwd`). In `terminal` mode the new terminal runs `claude --resume <id>` (or `claude -c` when only hooks know a session). In `cursor`/`vscode` mode the command is copied to the clipboard (`pbcopy`/`clip`/`xclip`), a popup says so, and the editor opens after ~600 ms. There is no keystroke injection: it needs macOS Accessibility permission and fails silently (osascript error 1002) without it. When a terminal is already open at the worktree, `Enter` (terminal mode) and `t` bring the terminal app forward instead. `c` always opens a terminal running the resume command. Supported terminals: Terminal.app, iTerm2, Ghostty and Warp (picked from `$TERM_PROGRAM`), and `x-terminal-emulator` on Linux.
8. **Worktree lifecycle (`am_core::worktrees`, called from `am-tui/src/ops.rs` and `am-cli/src/commands/lifecycle.rs`):** worktrees live at `<repo>/.claude/worktrees/<branch with / → ->`. Create = limit check → existence check (`CreateMode::Fresh` only; returns `BranchExists{local,remote}` so the TUI can show the choices) → optional local-branch delete → fetch → `git worktree add` from `origin/<base>` when it exists → sync → nickname. Modes: `Fresh`, `ReuseLocal` (`--reuse`, fast-forwards to origin first), `TrackRemote` (`--track`), `Disconnected` (`--no-track`), `DeleteAndRecreate`. The New Worktree form runs a debounced (400 ms) **advisory** live check (`check_branch`): "✓ exists on origin — Enter will offer to pull it" hides the base field, since the worktree will track `origin/<branch>`; "! exists locally only" offers reuse. Remote existence goes through `remote_branch_probe`, which races `git ls-remote` against a `gh api` ref lookup. The first "yes" wins, and a timeout or failure on one path can't produce a false "no". Delete handles a worktree, a main checkout on a non-default branch (branch only), or a stale row whose directory is gone; the TUI deletes with `force` after showing dirty/unpushed warnings. `check_worktree_limit` (`worktreeLimitEnabled` + `maxWorktrees`, per repo, excluding the main checkout) hard-blocks creation in both the TUI and the CLI. After creating, the TUI offers to run the repo's startup script (`scripts/<repo-id>.sh`) in the real terminal.

### Key Modules
`am-core` (`crates/am-core/src/`):
- `paths.rs` — `app_dir()` = `$AM_DATA_DIR` or `~/.agent-monitor`; DB, settings, log and `hook-captures/` paths derive from it.
- `log.rs` — append-only `debug.log` in `[ISO] [LEVEL] [module] message` format; `info!`/`warn!`/`debug!` macros; one line per call; rename-based rotation.
- `db.rs` — WAL, `busy_timeout` 5 s, `journal_size_limit` 4 MB, `PRAGMA user_version` migrations (`SCHEMA_VERSION` = 2: v1 is a baseline of the 1.x schema, v2 adds `subagents`). Tables: `repositories`, `worktrees`, `agent_status`, `standalone_sessions`, `subagents`, `integration_cache` (`managed_sessions` is a 1.x leftover kept in the baseline). Holds all dashboard queries plus `data_version()`.
- `status.rs` — `Status`, `HookEvent::from_json`, origin-aware `map_event_to_status`, `detect_git_activity`, `summarize_tool_input`.
- `settings.rs` — `Settings` (camelCase serde, defaults mirror 1.x), parsed field by field so one bad value can't reset the rest; unknown keys are kept in `raw`; `save` keeps the file's key order (serde_json `preserve_order`) and writes via temp file + rename.
- `model.rs` — `Repository`, `Worktree`, `AgentState`, `GitStatus`, `CommitInfo`, `PrInfo`, `LinearInfo`, `Subagent`, `WorktreeView`, `WorktreeGroup`; serde shapes match the 1.x cache payloads.
- `grouping.rs` — `apply_filters` (`hideMainBranch`, `hideMergedClosedPrs`, `hideIdleDoneAgents`, `hideWithoutLinearTicket`; a row with an effectively-open session, meaning open and not idle for >10 min, is never hidden), `compare` over the ordered `worktreeSort` criteria (the first non-equal enabled criterion wins), `build_groups` (one group per repo, config order unless the `repo` criterion is enabled), `top_sort_key`, `is_open_now`. There is no separate grouping setting: ticket/project clustering comes from the sort order. Shared by the TUI and `am worktree list`.
- `gitinfo.rs` — `git status --porcelain=v2 --branch` + `git log -1` per worktree; `git worktree list` parsing.
- `stacks.rs` — stacked-branch inference (Data Flow 6).
- `worktrees.rs` — repo add/remove, sync with `git worktree list`, branch checks + remote probe, create/delete, worktree limit, startup scripts. Blocking; never call it from the UI thread.
- `integrations/` — `mod.rs` (the `Integrations` facade: `hydrate`, `refresh_github`, `refresh_linear`, `fields_for`, `health`; `FetchResult` tells absence from failure), `github.rs` (one `gh api graphql` per repo with aliased `pullRequests(headRefName:)`, 40 branches per call, checks from `checkRunCountsByState` plus normalised `StatusContext`s, falls back to per-branch `gh pr view` on a non-network GraphQL failure; `GhRunner` trait for fixtures), `linear.rs` (raw GraphQL over `ureq`, aliased `issueVcsBranchSearch` batches of 25 distinct branches, non-2xx or GraphQL `errors` = failure, `AM_LINEAR_API_URL` override; `HttpTransport` trait), `cache.rs`, `breaker.rs`.
- `procscan.rs` — one process-table snapshot per scan (libproc via `libc` on macOS, `/proc` on Linux, ~2 ms): `has_terminal` (interactive shell in the worktree), `open_ide` (a VS Code/Cursor window or its integrated terminal), `running_processes` (attributed to the most-specific worktree root, filtered by `runningProcessFilter`). Nested worktrees don't light up their parent.
- `launch.rs` — session discovery, `resolve_open_action`, `claude_command`, clipboard, editor/terminal launch.
- `hooks_installer.rs` — writes the 11 hook events into `~/.claude/settings.json`, replacing only am's entries (`is_am_hook_command`) and keeping everyone else's. Points at the native binary by absolute path (`$AM_HOOK_BIN` → `am-hook` next to the current exe → `$PATH` → `~/.cargo/bin`; `AM_NO_NATIVE_HOOK=1` falls back to `cat | am hook-event`). `PermissionRequest` is written in Claude Code's own `--managed` shape so reinstalling is idempotent. Native commands carry a `# am hook-event (native)` comment so 1.x installs still recognise them.
- `timefmt.rs` — SQLite UTC datetime parsing, relative/short ages (computed at draw time).

`am-hook` (`crates/am-hook/src/lib.rs`): `parse_hook_args` (`-w/--worktree`, `-e/--event`, `--managed`, `--flag=value`; unknown flags are ignored) and `run_hook_event`. `AM_HOOK_CAPTURE=1` saves the raw payload per event and origin to `hook-captures/`. A passive WAL checkpoint runs on `Stop`/`SessionEnd`.

`am-tui` (`crates/am-tui/src/`): `main.rs` (event loop, key mapping, audio, log tail, `$EDITOR`, startup script), `app.rs`, `ui.rs` (+ insta tests), `data.rs`, `launcher.rs`, `ops.rs` (create/delete/branch check/add repo, and the `f` feature request via `gh issue create` on `roylet-gs/agent-monitor`), `modals.rs`, `settings_ui.rs` (field list, per-key persist that keeps unknown keys, sort sub-editor), `setup.rs` (Editor → Claude hooks → Repositories → Integrations), `theme.rs`.

`am-cli` (`crates/am-cli/src/`): `main.rs` (clap tree), `commands/*.rs`, `output.rs` (table / key-value / `--json`, same layout as 1.x), `resolve.rs` (branch/path → DB rows; detects the repo from the CWD, including from inside a tracked linked worktree), `store.rs` (CLI-only queries).

### CLI Commands
`crates/am-cli/src/commands/`:
- `worktree.rs` — `worktree list|info|open` (`wt` alias; `ls` and `open` shortcuts). `list` refreshes the shared integration cache (batched) and sorts with `grouping::build_groups`
- `lifecycle.rs` — `worktree create|delete|sync`, `new` shortcut
- `agent.rs` — `agent attach` (`a` alias): `claude --resume` the worktree's latest session in this terminal
- `repo.rs` — `repo list|add|remove`
- `settings.rs` — `settings list|get|set|reset` (`worktreeSort` can't be set from the CLI; `reset` keeps unknown keys such as `setupCompleted`)
- `hooks.rs` — `hooks install|uninstall|status`, plus `install-hooks` / `uninstall-hooks`
- `pr.rs`, `linear.rs` — `pr|linear [target]`, `show`, `open`
- `script.rs` — `script edit|remove|show`
- `doctor.rs` — app dir, DB, settings, hooks, native hook binary, `gh`, `ttyd`, Linear key
- `logs.rs` — `logs [-n N] [-f] [--level] [--module] [--clear]`
- `status.rs` — `status --worktree <path> [--set <status>]`
- `tui.rs` — no subcommand / `setup`: exec `am-tui`
- `hook-event` — dispatched in `main.rs` before clap (see Entry Flow)

### Persistence
All data lives in `~/.agent-monitor/` (or `$AM_DATA_DIR`): `agent-monitor.db` (SQLite, WAL), `settings.json`, `debug.log` (+ `debug.log.1` after rotation), `scripts/<repo-id>.sh`, and `hook-captures/` (only with `AM_HOOK_CAPTURE=1`). Hook config lives in `~/.claude/settings.json`. The 1.x files `am.sock` and `sessions/` are no longer used.

The DB and `settings.json` are shared with 1.x in place, so never rename or drop a column or setting key. Add schema changes as a new `user_version` step in `db::migrate` (idempotent, in one transaction) and bump `SCHEMA_VERSION`. New settings take their default from `Settings::default()` when absent, and unknown keys must keep round-tripping.

### Data Isolation (`AM_DATA_DIR`)
`am_core::paths::app_dir()` honours `AM_DATA_DIR` and every other path derives from it, so any binary can run against a temp directory without touching real data. Tests set it, plus a temp `HOME` so `~/.claude` is isolated too. Other env vars: `AM_HOOK_BIN`, `AM_NO_NATIVE_HOOK`, `AM_HOOK_CAPTURE`, `AM_LINEAR_API_URL`, `AM_BINARY_DIR` (npm launcher), `AM_PROCSCAN_ROOTS` (procscan probe test only).

## Testing

### Rust tests
- `cargo test --workspace` runs everything; `cargo clippy --workspace --all-targets -- -D warnings` must be clean.
- Unit tests sit next to the code (`#[cfg(test)] mod tests`). Pure logic is tested directly: status rules, grouping/sort, settings parsing, cache/breaker rules, GitHub/Linear response parsing through the `GhRunner`/`HttpTransport` fixtures, procscan with fixture processes, and `worktrees` against real temp git repos.
- `am-tui`: `App` is driven directly with `Key`s; `ui.rs` renders to a ratatui `TestBackend` and compares against **insta** snapshots in `crates/am-tui/src/snapshots/`. After an intended UI change run `INSTA_UPDATE=always cargo test -p am-tui` and review the `.snap` diffs.
- Integration tests: `crates/am-hook/tests/hook.rs` replays real event sequences through the binary; `crates/am-cli/tests/cli.rs` runs `am` with temp `AM_DATA_DIR`/`HOME` and fake `gh`/editor/Linear on `PATH`.
- `log::log` is a no-op under `cfg(test)`, so tests never write the real `debug.log`. Tests that touch process-global env (`AM_DATA_DIR`) hold a lock.

### Docker E2E Tests
End-to-end tests run the real binaries behind ttyd in Docker with GitHub and Linear mocked, driven by Playwright. Use them to capture screenshots and test full flows.

**Containers** (`e2e/docker-compose.yml`, `e2e/Dockerfile`):
1. **`mock-api`** — `e2e/mock-api/server.ts` on port 4100, serving fixtures from `e2e/mock-api/fixtures/`.
2. **`app`** — a `rust-build` stage (`cargo build --release --locked -p am-tui -p am-cli -p am-hook`, BuildKit cache mounts, LTO off for speed) → `debian:bookworm-slim` with git, ttyd and the fake `gh` (`e2e/bin/gh`, which proxies to mock-api). `e2e/entrypoint.sh` seeds `/work` with a local bare `origin` (`main` plus an origin-only `feature/on-origin`) and a linked worktree `feature/to-delete`, runs `am repo add /work`, and writes settings (`setupCompleted`, `hideMainBranch: false`, Linear on with a mock key, `linearAutoNickname: false`). It then replays hook events through `am hook-event` (UserPromptSubmit → SubagentStart → subagent PreToolUse → Stop), leaving `main` **delegating** with one running subagent, and fires a hook on the untracked `/home/user/my-project` to create a standalone session. ttyd port **7681** serves `am-tui` on the seeded data dir; port **7682** runs `am-tui-fresh`, which wipes `AM_DATA_DIR`/`HOME` on every connection so the setup wizard always shows. ttyd spawns a new `am-tui` per connection, so `page.reload()` restarts the process.
3. **`tests`** — Playwright (Chromium, 1280×1080) with `TUI_URL`/`TUI_FRESH_URL`/`MOCK_API_URL`. Config `e2e/playwright.config.ts`: 45 s timeout, 1 retry, 1 worker, sequential (the specs share one DB).

**Helpers** (`tests/e2e/docker/helpers/`): `tui-page.ts` (`TuiPage`: `goto`, `getScreenText` from the xterm.js buffer, `waitForText`, `sendKey`, `type`, `screenshot(name)` → `tests/e2e/tmp/<name>.png`), `rust-tui.ts` (`RustTui extends TuiPage`: `open`, `openUrl(FRESH_URL)`, `lines`, `keyBar` (last row), `selectedRow` (the `│▌` row), `selectedName`, `selectRow(branch)`, `waitForGone`, `waitForMatch`, `createWorktree`, `press`; re-exports the mock client), `mock-api-client.ts` (`setupMock({ gh?, linear?, ghVersion?, outage? })`, and `resetMock()` for `beforeEach`).

**Specs** (numbered so they run in a deliberate order against the shared DB; each restores any setting it changes):

| Spec | What it tests |
|------|---------------|
| `01-dashboard.spec.ts` | `─Worktrees` pane with `N of M`, `work ───` repo header, key bar hints; j/k move the `▌` selection, `Home`/`G` jump to top/bottom; `w` toggles the log pane |
| `02-help.spec.ts` | `?` help overlay (keys + status legend) opens and closes |
| `03-settings.spec.ts` | `s` settings editor; toggling "Resume last session on open" persists across a process restart (then restored) |
| `04-pr-status.spec.ts` | PR column: open (`#42✓`), draft (`#43`) and no PR, via the batched `gh api graphql` path; the Linear PR attachment stands in when GitHub has no PR |
| `05-linear.spec.ts` | ENG-123 ticket column and `— ticket —` cluster header; making Linear project the top sort key in the sort editor renders `═ Dashboard Revamp ═` (headers take no selection index), then restores |
| `06-offline-cache.spec.ts` | Mock outage → cached PR/ticket stay, with `⚠ GitHub + Linear offline — cached` on the key bar; `r` after recovery clears it |
| `07-subagents.spec.ts` | Delegating `main`: `◆` glyph + `◆ 1 subagent` badge; rows without running subagents have no badge |
| `08-standalone.spec.ts` | `Other sessions ───` header with the hook-created standalone session row (executing `●`), selectable |
| `09-new-worktree.spec.ts` | `n` form: live `✓ exists on origin` + base `tracks origin/…`, Enter → "Branch exists" choices; creating a fresh worktree adds and selects its row; an empty name is refused |
| `10-delete.spec.ts` | Delete dialog text + cancel; the main checkout is refused; create → delete (incl. the local branch) end to end |
| `11-setup-wizard.spec.ts` | The seeded dir skips the wizard; a fresh dir (7682) walks Editor → hooks install → add `/work` → Integrations → dashboard |

**Writing a spec:**
```typescript
import { test } from "@playwright/test";
import { RustTui, resetMock, setupMock } from "./helpers/rust-tui";

test.beforeEach(async () => { await resetMock(); });

test("my feature works", async ({ page }) => {
  await setupMock({ gh: { number: 42, state: "OPEN" } }); // optional
  const tui = new RustTui(page);
  await tui.open();                   // waits for "─Worktrees" and "N of M"
  await tui.selectRow("main");
  await tui.sendKey("s");
  await tui.waitForText("Settings");
  await tui.screenshot("my-feature"); // tests/e2e/tmp/my-feature.png
});
```

**Mock API routes:** `POST /gh` (routed on args: `gh api graphql` with `-f bN=<branch>` aliases, `gh pr view`, `--version`), `POST /linear` (viewer query for key verification, aliased `issueVcsBranchSearch`), `POST /mock/setup` (`{ gh, linear, ghVersion, outage }`; `outage: true` fails both routes), `DELETE /mock/reset`, `GET /health`. New fixtures go in `e2e/mock-api/fixtures/`.

New specs are picked up by the `tests` image's `COPY` on rebuild. On this macOS setup `docker` may be Podman (`docker compose` shells out to `docker-compose` v2 against the Podman socket); the Buildah backend honours the cache mounts.

## Distribution & Release

- **npm** (GitHub Packages, `@roylet-gs` scope): `@roylet-gs/agent-monitor` is a small Node launcher (`npm/agent-monitor/bin/am.js`) that `spawnSync`s the native `am` from the matching optional dependency `@roylet-gs/agent-monitor-{darwin,linux}-{arm64,x64}` (template `npm/platform/package.json`; each holds `bin/am`, `bin/am-tui`, `bin/am-hook`, and `os`/`cpu` make npm install only the matching one). `AM_BINARY_DIR` overrides the lookup (CI's launcher smoke test uses it). Hooks call `am-hook` by absolute path, so Node is never on the per-event path. See `npm/README.md`.
- **Homebrew:** the repo is its own tap. `npm/agent-monitor.rb.tmpl` is rendered into `Formula/agent-monitor.rb` on each release: `brew tap roylet-gs/agent-monitor https://github.com/roylet-gs/agent-monitor && brew install agent-monitor`.
- **CI** (`.github/workflows/ci.yml`; runs on PRs to main and is reused by publish): clippy `-D warnings` + `cargo test --workspace` on ubuntu and macOS, then builds the binaries and runs `node npm/agent-monitor/bin/am.js --version` against them.
- **Publish** (`.github/workflows/publish.yml`; every push to main except its own `chore: release v…` commit): CI → compute the version → build release binaries for `aarch64`/`x86_64-apple-darwin` and `x86_64`/`aarch64-unknown-linux-gnu` → GitHub Release `vX.Y.Z` with `agent-monitor-vX.Y.Z-<platform>.tar.gz` + `SHA256SUMS` → publish the four platform packages, then the main package with pinned `optionalDependencies` → regenerate the formula, stamp `Cargo.toml`/`Cargo.lock`, commit `chore: release vX.Y.Z` and push.
- **Versioning rule:** the release version is the `[workspace.package] version` in `Cargo.toml` if it is greater than the published npm version, otherwise the next patch past what's published. Bump `Cargo.toml` yourself only for a minor or major release; patch releases happen automatically on merge. All crates use `version.workspace = true`, and each binary's `--version` prints it.
- The release profile uses LTO, one codegen unit, `strip` and `panic = "abort"`.

## Evidence Capture & PR Workflow

### Slash Commands
- `/capture-evidence` — Runs clippy, then spawns two parallel sub-agents: Agent A runs `cargo test --workspace`, Agent B runs `pnpm test:e2e` for Docker E2E tests and screenshots. Includes a coverage mapping (`crates/am-tui` modules → specs) so Agent B knows which specs cover the diff and can add specs for gaps.
- `/create-pr` — Idempotent PR creation: clippy + tests, detects an existing PR via `gh pr view`, runs `/capture-evidence`, then creates or updates the PR with the evidence.
- `/merge-main` — Merge the latest main into the branch, then clippy + tests, fix, push.
- `/suggest-changes` — Analyse `am logs` output against the `crates/` sources and propose concrete fixes.

### Scripts (`.claude/scripts/`)
- `upload-evidence.sh [evidence-dir]` — Uploads screenshots as assets to the `evidence-store` GitHub release via `gh release upload --clobber` (branch-prefixed names) and prints the asset URLs. Called with `tests/e2e/tmp`.
- `seed-evidence-data.sh` — **Deprecated.** Local manual debugging only (isolated `AM_DATA_DIR` + `am repo add .`). Use `pnpm test:e2e` instead.
- `capture-tui.sh` — **Deprecated.** Local manual debugging only (serves a command such as `am-tui` via ttyd). Use `pnpm test:e2e` instead.
- `cleanup-tui.sh` — **Deprecated.** Local manual debugging only. Use `pnpm test:e2e:clean` instead.

### GitHub API Note
`gh pr edit --body` triggers a "Projects (classic) is being deprecated" GraphQL error. Use the REST API instead:
```bash
gh api "repos/OWNER/REPO/pulls/NUMBER" -X PATCH -f body='...'
```

## Documentation

When making changes to architecture, data flow, modules, or conventions, update this file (CLAUDE.md) and README.md to keep them in sync with the codebase. Status-rule changes also go in `STATUS_MAPPING.md`.

## Logging

When adding or modifying code paths, add log calls with the `am_core` macros (`use am_core::{debug, info, warn};`; the first argument is the module tag):
- `info!("module", ...)` for key state changes (status updates, hook install/uninstall, worktree create/delete, settings changes, integration recovery)
- `debug!("module", ...)` for branching decisions, mapping logic and timings (e.g. why `map_event_to_status` returned no change). `debug!` checks the level before formatting, so it is cheap when disabled.
- `warn!("module", ...)` for recoverable errors or unexpected state. Don't WARN on every cycle of a repeating failure; log the transition (see `breaker.rs`).
- Always include context (event names, tool names, paths, computed values, elapsed times).

Every call becomes exactly one line (`[ISO] [LEVEL] [module] message`; embedded newlines are joined with ` ⏎ `), written with a single `O_APPEND` write so concurrent hook processes never interleave. Each binary calls `log::init(level, maxLogSizeMb)` at startup, which rotates by renaming `debug.log` → `debug.log.1` once it exceeds `maxLogSizeMb`. Never print to stdout/stderr from `am-tui` (it owns the terminal) or from the hook path.

## Conventions

- Rust edition 2024, resolver 3; shared dependency versions in `[workspace.dependencies]`; every crate inherits the workspace version.
- `cargo clippy --workspace --all-targets -- -D warnings` must pass, and `cargo test --workspace` must stay green on Linux and macOS.
- **No async runtime.** Concurrency is `std::thread` + `std::sync::mpsc` channels (collector, integrations, launcher/ops jobs, scoped threads for parallel git). Blocking work never runs on the UI thread.
- The hook path must stay fast and infallible: no clap/TUI code in `am-hook`, one SQLite connection, no git spawns, always exit 0.
- One SQLite DB is shared by every process (hooks write while the TUI reads): WAL, busy timeout, `PRAGMA user_version` migrations. Keep the schema readable by 1.x (additive changes only).
- External tools are shelled out to (`git`, `gh`, `osascript`/`open`, `pbcopy`) rather than reimplemented, so the user's auth and config apply. Wrap them in traits where tests need fixtures.
- Shared types live in `am_core::model`. Dashboard logic used by both the TUI and the CLI lives in `am_core` (`grouping`, `worktrees`, `integrations`) so they can't diverge.
- `am-tui` keeps state and key handling pure (`app.rs`, `modals.rs`, `settings_ui.rs`, `setup.rs`) and side effects in `main.rs`/`launcher.rs`/`ops.rs`/`data.rs`.
- Unix only (macOS + Linux); `std::os::unix` APIs are fine.
- Published to GitHub Packages as `@roylet-gs/agent-monitor` (plus the per-platform packages) and to the in-repo Homebrew tap.
