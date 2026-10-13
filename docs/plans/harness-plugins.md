# Harness plugins + multi-session worktrees

## Step 0 (first thing after approval): kick off "training mode" planning in a stacked worktree
- `git worktree add .claude/worktrees/feature-plugin-training-mode -b feature/plugin-training-mode feature/agent-plugins-change`. This stacks on this branch, and am's stack detection will show it under this one. I won't switch branches here.
- Copy this plan into the new worktree as `docs/plans/harness-plugins.md`, so the planner knows what it is building on.
- Start a resumable headless Claude session in that worktree in plan mode (`claude -p --permission-mode plan --output-format json "<prompt>"`, run in the background) and give the user the `claude --resume <session-id>` command to continue it there. Plan approval stays with the user.
- The planner's prompt carries the user's spec verbatim:
  - A **Training mode** button on the harness plugin settings page opens a new terminal with a pre-made prompt (copied to the clipboard). An agent listens to its own hooks, uses what it knows about its own harness plus am's logs, writes a plugin manifest, and adds/validates it via the `am` CLI.
  - The new plugin enters in a **draft** state, visible in the TUI wherever it's used. In settings the user can **publish** it or **edit** it. Editing opens a terminal with an edit prompt: review the plugin, the hook logs am collected, and the errors/logs gathered while the plugin was in use, then fix the plugin.
  - The goal: use AI to get an unsupported harness working.
- The prompt also gives starting points to evaluate:
  - `AM_HOOK_CAPTURE` / `hook-captures/` as the "listen to itself" mechanism (a per-harness capture-all install for drafts).
  - `am plugin add|validate|publish|edit` CLI commands.
  - A manifest `status: draft|published` field.
  - Reusing `launch::open_terminal` + the clipboard path.
  - Per-plugin normalisation errors logged under a `plugin:<id>` module so the edit prompt can pull them via `am logs --module`.

## Context

The recent agent-plugin work (`am_core::agents`, `AgentProvider`, manifests, Claude + Gemini built-ins) treated *the agent* as the pluggable unit. The research says the right unit is the **harness**: the CLI or IDE that runs the model. The harness is what fires hooks, writes transcripts, owns the resume command and shows up in the process table. The **model** is just an attribute of a session. Codex, Copilot, Cursor, OpenCode, Pi, Droid and Qwen can all run many models, and Claude Code can sit behind a gateway.

Survey of 15 harnesses (Oct 2026):
- **Claude's hook schema is the de facto standard.** Most harnesses send JSON on stdin with Claude-style events and `session_id`/`cwd`/`transcript_path`, sometimes renamed. This includes Codex, Copilot CLI, Droid, Qwen, Gemini (renamed events), Cursor, Cline, Goose and Kiro. So keeping Claude's vocabulary as the canonical `HookEvent` is right, and most harnesses can be a manifest.
- **Integration comes in three shapes:**
  - (a) Shell command hooks in a settings file, but the file layouts differ: Claude nested matchers, Cursor/Copilot/Windsurf `{version,hooks:{event:[{command}]}}`, Codex `hooks.json`/TOML, Cline one executable per hook.
  - (b) In-process extension APIs only, with no shell hooks: OpenCode, Pi, Amp. These need a tiny bridge plugin that calls `am-hook`.
  - (c) No events at all: Aider. Only the process scan and transcripts are available.
- **Capabilities vary.** Some harnesses have no subagent events, no turn-end event (Crush, Windsurf) or no session-start event, so the status rules have to depend on capabilities.
- **No cross-harness standard to consume.** ACP only covers sessions you host, OTEL vocabularies diverge, and Agent Plugins 1.0 leaves hooks client-specific.

Separately, the dashboard has never handled **several sessions in one worktree**: `agent_status` is keyed by `worktree_id`, so the last writer wins. That happens with two harnesses, and also with two Claude sessions on the same branch. The user wants this re-thought (not merged into one status), and wants the model recorded and shown.

Outcome: am works with any harness, several at once and several per worktree. A new harness is a plugin (usually a JSON manifest, occasionally a Rust built-in).

## Design

### 1. Vocabulary: agent → harness (code), keep persisted names
- Rename the module `am_core::agents` → `am_core::harness`, `AgentProvider` → `Harness` and `AgentKind` → `HarnessKind`. This is mechanical, done in its own commit.
- **Keep** the DB column `agent`, the hook flag `--agent` (add a `--harness` alias), the settings keys `defaultAgent`/`disabledAgents`, and the `<app>/agents/*.json` manifest dir (also read `<app>/harnesses/`). This follows the 1.x compatibility rule and keeps already-installed hook commands working.
- UI copy changes: "Agent plugins" → "Harnesses", and the setup-wizard step becomes "Harness hooks".

### 2. Sessions as first-class rows (schema v4)
- New table `sessions`:
  - Key: `(harness, session_id)`.
  - Location: `worktree_id` (nullable) and `standalone_path`.
  - State: `status`, `model`, `cwd`, `transcript_path`, `last_response`, `last_tool`, `last_tool_input`, `is_open`, `started_at`, `updated_at`, `ended_at`.
  - Index on `(worktree_id, is_open)`.
  - Add `harness` to `subagents`, since session ids can collide across harnesses.
- Hook path (`crates/am-hook/src/lib.rs`):
  - Upsert the session row, and run `map_event_to_status` against **that session's** previous status, not the worktree's.
  - Then mirror a "headline" into `agent_status`/`standalone_sessions` so 1.x and older readers still work.
  - Headline priority: waiting > executing/thinking > delegating > done > idle, newest first within a level.
- Session liveness:
  - `SessionEnd` closes the session.
  - The process scan closes open sessions whose harness has no live process in the worktree (via `matches_process`). This is needed for harnesses without `SessionEnd`.
  - The existing "open and not idle for more than 10 min" rule still applies.
- Model: `WorktreeView.sessions: Vec<SessionView>` (harness, model, status, context, subagents, last tool, updated). `agent`/`status()` become derived from the headline session, so grouping, filters and sort keep working (`crates/am-core/src/model.rs`, `grouping.rs`).
- **UI** (`app.rs` `Row`, `ui.rs`):
  - With 0 or 1 live sessions, the row is unchanged. It gains a dim model label, e.g. `codex · gpt-5`, after the harness badge (no badge for the default harness, as today).
  - With 2 or more live sessions, the worktree row shows the headline glyph plus a `⧉ N sessions` badge. Below it is one `Row::Session(wt, s)` per live session (`┆` prefix, glyph, harness badge, model, last tool/age, ctx bar).
  - Session rows are **selectable**: `Enter`/`c` resumes *that* session with its harness. The `flat == visual order` invariant is extended to session rows.
  - Unseen markers and audio cues work per session.
  - New key `C`: start a new session in a chosen harness (picker over enabled harnesses).
- CLI:
  - `am worktree info` and `list --json` gain `sessions[]`.
  - New `am session list [--json]`.
  - `am agent attach [--session <id>]`.

### 3. The harness contract (trait + manifest)
Extend `Harness` and the manifest (`crates/am-core/src/agents/manifest.rs`) with:
- **`capabilities`**: `sessionEvents`, `turnEnd`, `permissionEvents`, `subagents`, `model`. `status.rs` uses them. With no `turnEnd`, a session falls to `done` after the harness process exits or after N s with no events. With no `sessionEvents`, the session opens on its first event.
- **`integration.kind`**:
  - `commandHooks` with `format`: `claudeSettings` (today), `flatHooksJson` (Cursor, Copilot, Windsurf, Droid), `codexHooks`, `scriptDir` (Cline).
  - `commandHooks` also gets `hermesYaml`, and every format allows "no `projectDirVar`" (use the payload `cwd`).
  - `bridgePlugin`: am writes a small embedded JS/TS shim into the harness's plugin dir. The shim spawns `am-hook hook-event --agent <id> --event <E>` with the event JSON on stdin (OpenCode, Pi; Amp later).
  - `none`: process and transcript only (Aider).
- `hooks_installer.rs` becomes format-driven: a `HookWriter` per format implementing install/uninstall/is_installed/dangling detection. The existing marker (`is_am_hook_command`) and `repair_dangling_hooks` logic are reused per writer.
- **Normalisation upgrades** (data, not code):
  - `fields` take JSON-pointer paths, e.g. `cwd ← /workspace_roots/0` and `session_id ← /conversation_id`.
  - Per-tool `toolInput` key maps, so `summarize_tool_input` works across harnesses.
  - New `model` field.
  - `projectDirVar` becomes optional: am-hook falls back to the payload `cwd` to find the worktree.
- **Session discovery as data**: `sessions.glob` (`~` / `{root}` aware) plus JSON pointers for `id`/`cwd`/`modified` read from the first lines of each JSONL. This covers Codex rollouts, Droid and Copilot `events.jsonl`. A Rust override stays available for SQLite stores (Goose, OpenCode).
- **Context usage per harness**:
  - Move `context.rs` under the Claude built-in.
  - Add a manifest `usage` spec: JSON pointers for input/cache tokens and model on the last matching JSONL line.
  - Fix the bypass at `crates/am-tui/src/data.rs:393`, where standalone sessions always read Claude transcripts.
- Remove the remaining Claude-isms outside the module:
  - `procscan::claude_shell_commands` → a harness `shellMarker`.
  - `HANDBACK_TOOL` → a capability.
  - The `AgentKind::CLAUDE` special-cases in `detected()`, `plugins_ui.rs:63`, `hooks.rs`, `doctor.rs` and `ui.rs:967` → "default harness".
  - Drop the dead `agentClaudeArgs`/`agentPermissionMode` reads.

### 4. Built-in harness plugins (embedded manifests like `gemini.json`, Rust only where needed)
| Harness | Integration | Notes |
|---|---|---|
| Codex CLI | `codexHooks` (`~/.codex/hooks.json`) | `codex resume <id>`/`--last`; rollout JSONL glob; model in payload |
| Copilot CLI | `flatHooksJson` (`~/.copilot/hooks/am.json`, PascalCase mode = Claude payload) | `copilot --resume` |
| Qwen Code | `claudeSettings` (`~/.qwen/settings.json`) | Gemini fork, has subagent events |
| Factory Droid | `flatHooksJson` (`~/.factory/hooks.json`) | `droid --resume <id>` |
| Cursor (IDE + `cursor-agent`) | `flatHooksJson` (`~/.cursor/hooks.json`, camelCase events, `conversation_id`, `workspace_roots`) | the CLI may fire only shell hooks; a `notes` caveat until verified |
| OpenCode | `bridgePlugin` (`~/.config/opencode/plugins/agent-monitor.ts`) | `opencode --session <id>`; Rust session discovery |
| Pi (`earendil-works/pi`) | `bridgePlugin`: TS extension at `~/.pi/agent/extensions/agent-monitor.ts`, which spawns `am-hook` with the payload on stdin via `node:child_process` | Events `session_start/shutdown`, `input`, `tool_call/tool_result`, `agent_end` (turn end), `ui_prompt_start` (waiting). No subagents. Sessions glob `~/.pi/agent/sessions/*/*.jsonl` (header has `id`/`cwd`); per-message `usage` + `model` → context bar. `pi --session <id>` / `--continue`. Process is `node`, matched by argv |
| Hermes (Nous Research) | `commandHooks` with a new `hermesYaml` format: a `hooks:` block in `~/.hermes/config.yaml` (YAML, edited preserving the rest of the file), absolute paths (no `$VAR` expansion), payload `cwd` locates the worktree | First-run consent: also write our commands into `~/.hermes/shell-hooks-allowlist.json` (or tell the user), and `doctor` explains it. Has subagent + approval events. Sessions in SQLite `~/.hermes/state.db` (Rust discovery, model + tokens). `hermes -r <id>` / `-c`. Python process, matched by argv |

Each is verified by capturing real payloads (`AM_HOOK_CAPTURE=1`) and committing them as fixtures that `crates/am-hook/tests/hook.rs` replays.

## Delivery (separate PRs, each green on its own)
1. Rename + contract cleanup + the context bypass fix (no behaviour change).
2. `sessions` table + multi-session UI/CLI + model field.
3. Capabilities, integration kinds/formats, normalisation and discovery upgrades.
4. Built-ins: wave A (Codex, Copilot, Qwen, Droid: manifest-only), wave B (Cursor, Hermes YAML + SQLite), wave C (OpenCode + Pi bridge plugins).

Open risks to check against live installs before each plugin lands:
- Does `cursor-agent` now fire every hook event?
- Hermes `state.db` columns (is there a cwd column?).
- Node/Python argv shapes for process detection.
- Whether OpenCode stores sessions in JSON files or SQLite.

## Critical files
- `crates/am-core/src/agents/{mod,manifest,claude,gemini}.rs` → `harness/`
- `crates/am-core/src/{db,model,status,grouping,context,procscan,launch,hooks_installer,settings}.rs`
- `crates/am-hook/src/lib.rs`
- `crates/am-tui/src/{data,app,ui,settings_ui,plugins_ui,setup,launcher}.rs`
- `crates/am-cli/src/commands/{hooks,agent,worktree,doctor}.rs` + new `session.rs`
- Docs: `docs/agent-providers.md` → `docs/harnesses.md`, `CLAUDE.md`, `README.md`, `STATUS_MAPPING.md`

## Verification
- `cargo clippy --workspace --all-targets -- -D warnings`; `cargo test --workspace`.
- Unit tests:
  - Headline selection.
  - Per-session status transitions.
  - Capability fallbacks (no turnEnd → done on process exit).
  - JSON-pointer normalisation.
  - Each `HookWriter` round-trip, keeping foreign hooks and staying idempotent.
  - Glob session discovery against fixture dirs.
  - Migration v3→v4 on a 1.x-shaped DB.
- `am-hook` integration tests replay the captured fixtures per harness.
- Insta snapshots for a worktree with 2 sessions (claude + codex), with session child rows and model labels.
- E2E: a new spec `17-multi-session.spec.ts`. The entrypoint seeds a Claude session and a Codex session on `main` via `am hook-event --agent codex`. The spec checks the `⧉ 2 sessions` badge and both child rows, that selecting a session row works, and that `16-gemini` still passes. Add it to `e2e/affected-specs.sh`.
- Manual: install hooks for Codex and Claude, run both in one worktree, and watch both rows update.
