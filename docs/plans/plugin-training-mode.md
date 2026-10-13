# Harness plugin "training mode"

## Context

`docs/plans/harness-plugins.md` turns am's plugin layer into **harness** plugins: manifests with `capabilities`, `integration.kind`/`format`, JSON-pointer `fields`, `sessions.glob`, `usage`, and a multi-session `sessions` table. Even with that contract, someone still has to research each new harness's hook format, event names, payload keys, resume flags and process argv. Then they hand-write a manifest and verify it against real payloads. Today that is a developer job: `AM_HOOK_CAPTURE=1`, read the JSON, edit, restart.

Training mode hands that job to the harness itself. From Settings → Harnesses, the user starts a **training session**: a new terminal running the unsupported harness, with a pre-made prompt. The agent:
1. writes a first **draft** manifest from what it knows about its own harness (docs, `--help`, installed package files),
2. has am install a capture-everything hook set so am **records every event its own harness fires**,
3. triggers those events (a headless child run of itself, or a short session the user drives),
4. replays the captures through the manifest with `am plugin test`, fixes the mapping, and re-adds it until it validates clean.

The plugin then works in am in a visible **draft** state. While it is used, am collects per-plugin issues: unmapped events and tools, missing fields, failed discovery, unmatched processes. **Edit** opens a new training session with an edit prompt that points the agent at the manifest, the captures and the collected issues. **Publish** stops capturing and drops the draft badge.

What exists today (verified):
- `am-hook` (`crates/am-hook/src/lib.rs:83-88`) treats an **unknown `--agent` as Claude**, normalises the payload, then calls `maybe_capture` (:265). Capture is env-gated (`AM_HOOK_CAPTURE`) and writes `hook-captures/[<id>-]<canonicalEvent>-<main|subagent>.json`. That name uses the *normalised* event and overwrites the previous file, so it is useless for discovering native events.
- The registry (`agents::registry`, `OnceLock`) loads `<app>/agents/*.json` once per process. A running TUI never sees a new manifest.
- `hooks_installer` decides which entries are am's by command shape (`is_am_hook_command`), not by agent. Two plugins that share a settings file and an event name would overwrite each other.
- The TUI plugins page (`crates/am-tui/src/plugins_ui.rs`, effects in `settings_ui.rs:348`, drawn by `ui.rs:252 draw_plugins`, snapshot `agent_plugins_page`) supports space on/off, `i` install hooks and `d` default.
- `launch::open_terminal(path, Some(cmd))`, `launch::copy_to_clipboard`, and `launcher::Msg::{Popup,Notice}` cover launching.
- `am logs --module X` matches the substring `[X]`. The module tag is free-form.

## User flow

**Create**
1. Settings → Harnesses… → `T` (or the `＋ Train a new harness plugin…` row at the bottom of the list).
2. A modal asks for:
   - **Command**: how you start the harness, e.g. `cursor-agent` or `hermes`. Required, because am doesn't know unsupported harnesses.
   - **Name**: optional.
3. am prepares the training workspace `<app>/training/<slug>/`:
   - `git init` with one commit, so `am-hook` finds a `.git`. The session shows up under **Other sessions** and gives live feedback.
   - The rendered prompt goes to `TRAINING.md`.
   - A short README with a toy file the agent can read and edit during probes.
4. am copies a **one-line bootstrap** to the clipboard: `Read ./TRAINING.md in this directory and follow it exactly.` A single line is used because pasting a multi-line prompt into a TUI harness can submit on the first newline.
5. am opens a terminal there running `<command>`, and a popup says "Paste (⌘V) into <command> and press Enter".
6. The agent works through the prompt and ends with `am plugin add ./manifest.json`. The plugin lands as **draft**, and its hooks are installed, including the capture-only events.

**Draft in use**
- On worktree and session rows the badge becomes yellow `✦ mycli ✎ draft` (`✦✎` below 100 columns).
- The plugins page shows `draft` plus an issue count `⚠ 3`.
- `am doctor` lists drafts with their issue counts.
- Every hook event of a draft:
  - writes a capture (in a ring buffer),
  - logs one INFO line under `plugin:<id>`,
  - and normal status tracking applies, so the user sees whether it works.

**Edit loop**
- On the plugins page, `e` on a plugin row re-renders the **edit** prompt into `<app>/training/<id>/TRAINING.md`.
- It copies the bootstrap line and opens a terminal running the plugin's own launch command (`program`, plus `training.promptArgs` if the agent learned how to pass an initial prompt, in which case no paste is needed).
- Editing a published plugin first flips it back to draft, which restarts capturing. The agent fixes the manifest and re-adds it with `am plugin add --replace`. The previous version goes to the history.

**Publish**
- `p` on the plugins page asks for confirmation, then runs `am plugin publish <id>`. Publishing:
  - requires `validate` with no errors (warnings are shown in the confirm box),
  - removes the capture-only hooks,
  - sets `status: "published"`,
  - and keeps the captures and issues, which can be cleared with `am plugin captures --clear`.
- The agent is told **never** to publish. That is the user's call.

**Remove**
- `x` with a confirm, or `am plugin remove <id>`: uninstall its hooks, then move the manifest to the history.

## Data model / manifest

New optional top-level manifest keys (`crates/am-core/src/agents/manifest.rs`, `harness/manifest.rs` after the rename):
```json
"status": "draft",                       // "draft" | "published"; absent = published (existing files keep working)
"training": {
  "captureEvents": ["BeforeModel", "PreCompress"],   // extra native events installed ONLY while draft, never mapped
  "promptArgs": ["-i", "{prompt}"],                  // optional: start the harness with an initial prompt
  "verifiedWith": "cursor-agent 1.7.2",              // free text, shown in `show`
  "notes": "AfterTool lacks tool_input on errors"    // agent's own findings, carried into the edit prompt
}
```
- `status` lives **in the manifest**, so there is one source of truth that the agent, the CLI and the TUI all read. The manifest dir (`agents/`) is 2.x-only, so 1.x is unaffected. Older 2.x binaries will reject the new keys (`deny_unknown_fields`) and list the file as skipped, not crash. This is noted in docs.
- Built-ins are always `published`, and `e` on a built-in is refused ("copy to a plugin first" is out of scope).
- `Harness` trait: add `fn status(&self) -> PluginStatus` (default `Published`) and `fn capture_events(&self) -> Vec<String>`. `hook_events()` for a draft returns `events.keys() ∪ captureEvents`. Writers install both, and normalisation leaves capture-only events unmapped. That is safe because `map_event_to_status` ignores unknown events. Check that it does and add a test.
- **Registry reload**: replace the `OnceLock<Registry>` with `RwLock<Arc<Registry>>` plus `agents::reload()`. A reload leaks the new providers (`Box::leak`, as today). This is bounded because it only happens on a manifest change. `HarnessKind` stays `&'static`. The TUI collector reloads when the newest mtime in `agents_dir` changes (checked every 2 s with the process scan). Hooks are fresh processes, so they need nothing.
- **Unknown `--agent` on the hook path** (behaviour change):
  - Capture the raw payload to `hook-captures/<id>/`.
  - Log `plugin:<id>` WARN once per file-state (see dedupe below).
  - **Skip** the status write instead of treating the event as Claude's. That would corrupt a worktree's status when a draft manifest fails to parse.
  - Still exit 0.
- **Captures** move to per-plugin dirs: `<app>/hook-captures/<id>/<unix-ms>-<nativeEvent>.json`. Each file holds `{ "receivedAt", "nativeEvent", "argv", "cwd", "env": {…}, "payload": <raw> }`:
  - `nativeEvent` comes from `--event`, otherwise from the payload's `hook_event_name`, otherwise `unknown`.
  - `env` is an **allow-listed** subset, so no secrets: the names of every env var, plus the values of vars whose value is an existing directory. This is how the agent finds `projectDirVar`.
  - Payloads are capped at 256 KB (truncated, with a flag), and each plugin keeps a ring of 300 files (oldest deleted).
  - Captures are written for drafts always, for unknown ids always, and for anything when `AM_HOOK_CAPTURE=1`. Claude's legacy flat files keep working under `AM_HOOK_CAPTURE` for existing fixtures.
- **Issues (schema v5, additive)**: a `plugin_issues(plugin TEXT, kind TEXT, key TEXT, sample TEXT, count INTEGER, first_seen, last_seen, PRIMARY KEY(plugin, kind, key))` table, upserted with `count = count + 1`. Who writes which kind:

  | Writer | Kinds |
  |---|---|
  | Hook path (all manifest plugins, drafts and published) | `unmapped_event` (native event not in `events`, includes capture-only events so the agent sees what fired), `unmapped_tool` (tool not in `tools` and not a canonical tool name), `missing_field` (no `session_id` / no `cwd` after normalisation), `no_worktree` (no `.git` above the resolved dir), `bad_payload` (not JSON) |
  | Collector / launcher (TUI) | `discovery_failed` (sessions command or glob error), `discovery_empty` (hooks saw a session but discovery never lists it), `process_unmatched` (open session but no process ever matched `process`), `resume_failed` (terminal launch error), `manifest_error` (load error from `plugin_errors()`) |

  - Each issue logs WARN under `plugin:<id>` **only on first insert** (the transition rule); repeats only bump the count.
  - Cost on the hook path is one extra upsert, and only when something is wrong.
  - Built-ins never write issues.

## CLI (`crates/am-cli/src/commands/plugin.rs`, new; `am plugin`, alias `am harness` once PR 1 renames)

| Command | Behaviour |
|---|---|
| `plugin list [--json]` | id, origin, `draft`/`published`, enabled, hooks state, issue count, capture count |
| `plugin show <id> [--json]` | manifest, status, manifest path, capture dir, hook settings path + install state, training workspace, issue summary |
| `plugin schema [--json]` | prints the manifest reference (embedded `docs/harnesses.md` manifest section via `include_str!`) or the JSON Schema (hand-written `crates/am-core/src/agents/manifest.schema.json`; a test validates every built-in manifest and the doc example against it so it can't drift) |
| `plugin validate <file\|id> [--json]` | **all** problems, not just the first. Each has a JSON path, a message and a hint, for example `/hooks/settingsPath: ~/.claude/settings.json is already used by claude — two plugins can't share a hook file`. Errors fail the command (exit 1). Warnings are listed only: no turn-end mapping without `capabilities.turnEnd:false`; no `UserPromptSubmit`; no `Bash`/`AskUserQuestion` tool mapping; `program` not on `$PATH`; `configDir` missing; empty `process`; `sessions.command` present (runs arbitrary code); captures seen for events the manifest doesn't map; mapped events never seen in captures |
| `plugin add <file> [--replace] [--no-install]` | validate → copy to `<app>/agents/<id>.json` (forced `status: "draft"` unless `--replace` of a published one with `--keep-status`) → previous version to `<app>/agents/.history/<id>/<ts>.json` → back up the harness settings file once (`<file>.am-backup`, first install only) → install hooks (incl. capture events) → print next steps. Refuses to replace a built-in id |
| `plugin captures <id> [--event E] [--last N] [--full] [--json] [--clear]` | per-native-event summary (count, last seen, top-level keys seen), or full capture bodies |
| `plugin test <id> [--manifest <file>] [--capture <file>…]` | replays captures (default: all for `<id>`, in time order) through the **candidate** manifest (`--manifest`, so the agent iterates without re-adding), grouped by session. Per capture it prints `native → canonical`, tool mapping, extracted `session_id/cwd/model`, and the status transition `executing → done`. It ends with a coverage table: events mapped vs seen, tools unmapped, fields missing. Exit 1 if any capture fails to normalise. Pure, so nothing is written |
| `plugin issues <id> [--json] [--clear]` | the `plugin_issues` rows, newest first |
| `plugin publish <id>` / `plugin draft <id>` | flip status (`publish` needs zero validation errors); reinstall hooks so capture events are added or removed |
| `plugin remove <id> [--keep-captures]` | uninstall hooks, move the manifest to the history, clear issues |
| `plugin rollback <id>` | restore the newest history version, then reinstall |
| `plugin train [--command <cmd>] [--edit <id>] [--print] [--no-open]` | the same preparation as the TUI (workspace, `TRAINING.md`, clipboard, terminal). `--print` writes the rendered prompt to stdout. So training works without the TUI too |

`am doctor` gains a "Drafts" section and points at `am plugin issues <id>`. `am hooks install` keeps accepting draft ids.

The shared logic lives in `am_core::agents::training` (new): the workspace, rendering, validation report types, replay and capture IO. Both the TUI and the CLI call it, following the "can't diverge" convention.

## TUI

- **Plugins page** (`plugins_ui.rs`, `ui.rs draw_plugins`, effects `settings_ui.rs plugin_effect`):
  - Rows gain a status column (`draft` in yellow / blank) and `⚠ N` issues.
  - The last row is `＋ Train a new harness plugin…`.
  - The header lists the new keys: `T` train new, `e` edit/retrain, `p` publish (confirm), `x` remove (confirm).
  - The help line for a draft shows `captures N · issues N · am plugin issues <id>`.
  - New `PluginOutcome::{TrainNew, Edit(id), Publish(id), Remove(id)}`.
- **Train modal** (`modals.rs`): fields Command and Name. Enter → `launcher::spawn_training(Prepare)` on a thread → `Msg::Popup("Prompt copied — paste into <cmd> and press Enter")`, or an error popup. If no terminal can be opened, the popup gives the workspace path and the command to run.
- **Badges** (`ui.rs` row badges, `ctx`-style width rules): a draft harness gets `✎ draft` after `✦ <id>`, in yellow from the theme. Wide is `✦ mycli ✎ draft`, below 100 columns `✦✎`. The same applies to session child rows after PR 2. The help overlay explains `✎`.
- **Collector** (`data.rs`):
  - registry reload on manifest mtime change,
  - `process_unmatched`/`discovery_*` issue writes, at most once per (plugin, session) per process,
  - snapshot carries `draft_ids` and issue counts.

## Prompt templates

Embedded as `crates/am-core/src/agents/training/{create,edit}.md`. Placeholders are filled by `training::render`:
`{{am}}` (absolute `am` path, found the same way as `am-hook`), `{{app_dir}}`, `{{workspace}}`, `{{command}}`, `{{id}}`, `{{manifest_path}}`, `{{capture_dir}}`, `{{formats}}` (the supported `integration` kinds/formats, from code), `{{canonical_events}}`, `{{canonical_tools}}`, `{{example_ids}}` (the built-in ids to study).

### create.md
```markdown
# Teach agent-monitor (am) to monitor this harness

You are running inside a coding-agent harness started with `{{command}}`. am is a dashboard that tracks
harness sessions through hook events. It doesn't support yours yet. Your job: write and verify an am
harness plugin manifest for the harness you are running in. Work only in `{{workspace}}` and with the
`{{am}}` CLI. Don't change any other project.

## Rules
- Never edit am's files by hand (`{{app_dir}}`); use `{{am}} plugin …`. Never edit another harness's config.
- Never run `{{am}} plugin publish`. The user publishes.
- Keep probe runs tiny (one short prompt). Ask the user before anything that costs much or needs approval.
- If something is unknown, say so in `training.notes`. Don't guess silently.

## 1. Learn the contract
Run `{{am}} plugin schema` (the reference) and `{{am}} plugin show <id> --json` for the built-ins
{{example_ids}} as worked examples. Supported integrations: {{formats}}. Canonical events: {{canonical_events}}.
Canonical tools: {{canonical_tools}}.

## 2. Learn your harness
Use what you know about your own harness, then confirm it locally: `{{command}} --help`, its config dir,
its installed package files and docs, the web if you have it. Find:
- the hook mechanism (config file + format, or extension API), event names, and the stdin payload keys
  (session id, cwd, tool name/input, final response, notification/approval type, model, subagent ids),
- how to resume a session by id and continue the latest, and how to start it with an initial prompt,
- where sessions/transcripts are stored and how to read id/cwd/modified time (and token usage, if any),
- what the process looks like in `ps` (name and argv).
If the hook mechanism matches none of the supported integrations, stop: write what you found to
`{{workspace}}/FINDINGS.md`, tell the user, and suggest they file it with `f` in am.

## 3. Draft and listen to yourself
Write `{{workspace}}/manifest.json` with `"status": "draft"`. Map the events you are sure of in `events`.
List **every other event name your harness documents** in `training.captureEvents`, so am records them too.
Then:
    {{am}} plugin validate ./manifest.json
    {{am}} plugin add ./manifest.json
am now captures every hook event your harness fires into `{{capture_dir}}`. Most harnesses read hook
config only at session start, so this session probably won't fire them. Trigger them in a **new** session:
- Preferred: run your harness headless from `{{workspace}}`, e.g. a non-interactive mode with a prompt like
  "read README.md, run `ls`, then edit NOTES.txt", plus a turn that needs approval if your harness can do that headless.
- Otherwise ask the user to run `{{command}}` in a second terminal in `{{workspace}}`. Give them exact
  steps: send a prompt, let one shell tool run, approve one permission prompt, answer one question, exit.
Then read what arrived: `{{am}} plugin captures <id>` and `{{am}} plugin captures <id> --full --last 5`.
`env` in each capture lists the variables the hook saw; a directory-valued one is your `projectDirVar`.

## 4. Fix until clean
Edit `manifest.json`, then replay the captures through it without reinstalling:
    {{am}} plugin test <id> --manifest ./manifest.json
Aim for: every captured event either mapped or deliberately left in `captureEvents`; a full turn that goes
executing → done (or waiting on approval); tools mapped (`Bash`, `Read`, `Edit`, `Write`, `AskUserQuestion`…);
`session_id` and `cwd` found; sessions discovered; `resume` correct. Check discovery and resume yourself
from `{{workspace}}`. Re-add with `{{am}} plugin add ./manifest.json --replace` and repeat step 3 if you
changed hooks. Also check `{{am}} plugin issues <id>` and `{{am}} logs --module plugin:<id> -n 50`.

## 5. Report
Record what you verified and what you couldn't in `training.verifiedWith` / `training.notes`, run
`{{am}} plugin validate <id>` one last time, and give the user a short summary: what works, what doesn't,
and that the plugin is a **draft** they can publish from am's Settings → Harnesses.
```

### edit.md
```markdown
# Improve the am harness plugin `{{id}}`

You are running inside the harness that plugin `{{id}}` describes. The plugin is a **draft** in
agent-monitor (am). It has been in use, and am has collected what went wrong. Fix the plugin. Work in
`{{workspace}}` with the `{{am}}` CLI only. Same rules as before: don't edit am's files or another
harness's config by hand, don't publish, keep probe runs tiny.

## 1. Gather the evidence
    {{am}} plugin show {{id}}
    {{am}} plugin issues {{id}}
    {{am}} plugin captures {{id}}
    {{am}} plugin test {{id}}
    {{am}} logs --module plugin:{{id}} -n 200
    {{am}} plugin schema
Current manifest: `{{manifest_path}}`. The previous agent's notes: `training.notes` in it.
Issue kinds: `unmapped_event`, `unmapped_tool`, `missing_field`, `no_worktree`, `discovery_failed`,
`discovery_empty`, `process_unmatched`, `resume_failed`, `manifest_error`. Each has a sample. Start with
the highest counts.

## 2. Diagnose
For each issue, find the cause in the captures (`--full`) and in what you know about your harness. If an
issue needs fresh evidence (e.g. an event that has never fired), trigger it the same way as during
training: a headless run from `{{workspace}}`, or exact steps for the user. Check whether your harness
version changed (`{{command}} --version` vs `training.verifiedWith`).

## 3. Fix and verify
    cp {{manifest_path}} ./manifest.json     # edit this copy
    {{am}} plugin test {{id}} --manifest ./manifest.json
    {{am}} plugin validate ./manifest.json
    {{am}} plugin add ./manifest.json --replace
After replacing, clear issues you have fixed (`{{am}} plugin issues {{id}} --clear`) so new ones stand out,
and trigger a short session to confirm nothing new appears. `{{am}} plugin rollback {{id}}` restores the
previous version if you make things worse.

## 4. Report
Update `training.notes`/`verifiedWith`. Tell the user what you fixed, what is still open, and that they can
publish from Settings → Harnesses when they are happy.
```

## Capture / feedback mechanism (summary)
- **Listen to itself**:
  - Draft hooks = mapped events ∪ `training.captureEvents`, installed through the PR 3 `HookWriter` for the manifest's format.
  - `am-hook` captures every draft event natively named, with argv, cwd and env names, in a ring buffer per plugin.
  - The agent reads them back with `am plugin captures` and checks its mapping offline with `am plugin test`.
  - The agent triggers events through a **headless child run of its own harness** in the workspace, or through exact steps for the user. A new process is needed because most harnesses load hooks at startup.
- **Use feedback**: the `plugin_issues` table (aggregated, bounded) plus first-seen WARN lines and per-event INFO lines for drafts under `plugin:<id>`. Both are read by the edit prompt.
- **Hot path safety**:
  - Captures and issue writes are best-effort, after normalisation, inside the existing always-exit-0 wrapper. There is no extra spawn, and IO is bounded (256 KB cap, ring of 300).
  - An unknown or broken plugin id no longer falls through to Claude's mapping.
  - `validate` refuses a `settingsPath`/format target that another registered plugin already writes, so a draft can't wipe Claude's or Gemini's entries.
  - The first install backs up the target settings file.
  - Remove and rollback go through the existing "only am's entries" uninstall.
  - Add a timing test: draft path < 10 ms on fixture payloads.

## Dependencies on the harness-plugin PRs

| Training piece | Needs |
|---|---|
| Naming (`am harness` alias, `harness::training`, "Harnesses" page copy) | PR 1 (rename). Training can start right after PR 1 |
| `status`/`training` manifest keys, registry reload, per-plugin captures, unknown-id fix, CLI `add/validate/captures/test/issues/publish/remove/rollback/train`, plugins-page actions, row badge | PR 1 only, on today's `claudeSettings` format |
| Capture events for non-Claude-shaped harnesses (`flatHooksJson`, `codexHooks`, `scriptDir`, `hermesYaml`); optional `projectDirVar` (cwd fallback); JSON-pointer `fields`; `sessions.glob`; `usage`; `capabilities` in validation warnings; `{{formats}}` in the prompt | PR 3. Without it training can only produce Claude-shaped manifests, so **most unsupported harnesses need PR 3** |
| Bridge-plugin drafts (OpenCode/Pi-style): the shim forwards *every* extension event when the plugin is a draft | PR 3 `bridgePlugin` + wave C shims. A manifest can't ship new shim code, so a harness with a new extension API still ends in FINDINGS.md |
| Draft badge on session child rows, `model` in captures/test output, `discovery_empty` per session | PR 2 (`sessions` table) |
| Worked examples in the prompt (`{{example_ids}}`) | PR 4 waves (Codex/Copilot/Qwen/Droid are the best examples). Before that, only Gemini |
| `plugin_issues` = schema **v5** | after PR 2's v4 |

**Recommended delivery** (each green on its own, after PR 3 lands; T1 can start after PR 1 if wanted):
- **T1 core + CLI**: manifest keys, trait additions, registry reload, captures v2, unknown-id fix, `plugin_issues` (v5), `training` module (render, workspace, replay, validation report), `am plugin …`, doctor.
- **T2 TUI**: plugins page actions + modal + launcher, badges, collector issues/reload, help text.
- **T3 prompts + docs + E2E**: templates, `docs/harnesses.md` "Training a harness" section, CLAUDE.md/README, spec 18.

## Critical files
- `crates/am-core/src/agents/{mod,manifest}.rs` (→ `harness/`), new `agents/training/{mod.rs,create.md,edit.md}`, new `agents/manifest.schema.json`
- `crates/am-core/src/{db.rs (v5), paths.rs (capture_dir/<id>, training_dir, history dir), hooks_installer.rs (capture events, backup, clash check)}`
- `crates/am-hook/src/lib.rs` (`maybe_capture` v2, unknown-id path, issue upserts, draft INFO log)
- `crates/am-cli/src/main.rs` (`PluginCmd`), new `commands/plugin.rs`, `commands/doctor.rs`
- `crates/am-tui/src/{plugins_ui,settings_ui,modals,launcher,data,ui}.rs`
- Docs: `docs/harnesses.md`, `CLAUDE.md`, `README.md`

## Testing
- **Unit (am-core)**:
  - manifest parse with `status`/`training`, and absent → published;
  - draft `hook_events` includes capture events, published doesn't;
  - capture-only events map to no status change;
  - validation reports *all* errors with paths and hints, including the settingsPath clash and warnings;
  - schema file validates every built-in manifest;
  - `render` fills every placeholder (no `{{` left) for both templates;
  - replay over fixture captures gives the expected transitions and coverage table;
  - ring buffer prunes to 300;
  - env capture keeps only names plus directory values;
  - registry `reload` picks up added, changed and removed files;
  - issue upsert counts and first-seen WARN once.
- **am-hook integration** (`tests/hook.rs`):
  - a draft plugin event writes `hook-captures/<id>/…-<Native>.json` with no stdout;
  - an unknown `--agent` captures, writes no status and exits 0;
  - an unmapped tool or missing session_id adds an issue row;
  - a published plugin doesn't capture without `AM_HOOK_CAPTURE`.
- **CLI** (`tests/cli.rs`):
  - `add` → `list` shows draft → `test --manifest` output → `publish` refused while errors remain → fix → `publish` → `rollback`;
  - `train --print --command mycli` contains the workspace and the am path;
  - `--json` shapes;
  - `doctor` lists drafts.
- **Insta**:
  - plugins page with a draft + issues + the `＋ Train` row;
  - train modal;
  - a worktree row and a standalone row with `✦ mycli ✎ draft` at 120 and 90 columns.
- **E2E** `18-plugin-training.spec.ts`:
  - The entrypoint drops a draft manifest `e2e/fixtures/agents/toycli.json` and replays a few `--agent toycli` events (one unmapped tool).
  - The spec checks the `✎ draft` badge on the standalone row and that the plugins page shows `draft ⚠ 1`.
  - `T` opens the modal; Enter shows the "prompt copied" popup. The container has no terminal or clipboard, so assert the error/fallback popup text with the workspace path, plus `TRAINING.md` existing via `docker exec`, or via a `--no-open` test hook.
  - `p` → confirm → the `draft` tag goes away (then restored).
  - Add `18` to the rows for `agents/`, `plugins_ui.rs`, `am-hook` and `plugin.rs` in `e2e/affected-specs.sh`.
- **Manual**: train a real unsupported harness end to end (e.g. Hermes or `cursor-agent` once PR 3 is in), then edit after a day of use.

## Open questions for the user
1. **CLI noun**: `am plugin …` (the spec's word) or `am harness …` to match PR 1's rename? Proposal: `am harness`, with `plugin` as an alias.
2. **Who triggers events**: is it OK for the training agent to spawn a **headless child run of its own harness** (costs a few tokens and may need permission flags), or should it always ask the user to drive a second session?
3. **Editing a published plugin**: flip it back to draft automatically (capturing resumes and the badge reappears), or keep it published while being edited?
4. **Should published manifest plugins keep collecting issues** (cheap, and useful later), or only drafts?
5. **Clipboard + paste** vs passing the prompt as an argument: for create, am can't know the harness's prompt flag, so the plan uses the one-line bootstrap. Is a paste step acceptable, or should the modal take an optional "initial prompt flag" (e.g. `-i`)?
6. **Drafts and the rest of am**: may a draft be the `defaultAgent` (used by `c`), or only after publishing?
7. **Sharing**: out of scope here, but should publish later offer "open a PR/issue to upstream this manifest as a built-in" (reusing `f` / `gh issue create`)?

## Implementation notes (built on PR 1, `feature/agent-plugins-change` / #141)
Where the build differs from the plan above:
- **CLI noun:** `am harness …`, with `am plugin …` as an alias (open question 1, proposed default).
- **Defaults taken for the open questions:**
  - the training agent may run its own harness headless;
  - editing a published plugin turns it back into a draft;
  - published plugins record issues too (only drafts capture);
  - create mode uses the one-line paste;
  - drafts may be the default agent;
  - upstreaming is left for later.
- **Badge:** `✎ draft`, not `◇draft`, because `◇` is already the *planning* status glyph.
- **Schema:** `plugin_issues` is **v4** on this branch, since nothing else has taken v4 yet. Whichever of this and PR 2's `sessions` table lands second renumbers its step.
- **Train UI:** the command input and the publish/remove confirmations live inside the plugins page (`plugins_ui.rs`), not in `modals.rs`.
- **Issue kinds built:** `unmapped_event`, `unmapped_tool`, `missing_field`, `no_worktree`, `bad_payload`, `unknown_plugin`, `discovery_failed`. `no_worktree` only fires when there is no directory at all (empty `--worktree` and no payload `cwd`). A session outside git is a legitimate standalone session, so it isn't an issue.
- **Issue kinds not built yet:**
  - `discovery_empty` and `process_unmatched` need PR 2's per-session rows.
  - `resume_failed`: am can't observe a resume failing in a detached terminal.
- **Still needs PR 3:** capture events for non-Claude-shaped hook formats, optional `projectDirVar` (the hook already falls back to the payload `cwd` when `--worktree` is empty), and JSON-pointer fields.
