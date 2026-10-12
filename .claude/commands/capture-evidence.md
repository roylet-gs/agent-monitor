# Capture E2E Evidence

Collect lint, test, and visual evidence that the current changes work. Uses the Cargo test suite plus the Docker-based E2E infrastructure (real `am` / `am-tui` / `am-hook` binaries behind ttyd, driven by Playwright) for screenshots and integration tests.

**IMPORTANT: You MUST complete ALL steps below. Do NOT skip steps, cut corners, or summarize. Every agent MUST be spawned. Every result MUST be collected.**

## Steps

1. **Lint check** — Run `cargo clippy --workspace --all-targets -- -D warnings` and capture the output. If it fails, stop and report.

2. **Analyze diff** — Run `git diff main...HEAD --name-only` to identify changed files. Use the coverage mapping table below to determine if new E2E specs are needed.

3. **Spawn ALL parallel sub-agents** using the Agent tool. You MUST launch all applicable agents in a SINGLE message with PARALLEL tool calls. Do NOT run them sequentially. Do NOT skip any agent.

   **Agent A: Rust tests** (REQUIRED) — Run `cargo test --workspace`. Capture per-crate / per-test-binary pass/fail counts (am-core, am-hook incl. `tests/hook.rs`, am-tui incl. insta snapshots, am-cli incl. `tests/cli.rs`) and list any failing tests by name. If an insta snapshot fails, include the snapshot diff; do NOT accept snapshots (`INSTA_UPDATE=always`) unless the UI change is intended by the diff, and say so if you did. Return structured results as a table.

   **Agent B: Docker E2E + screenshots** (REQUIRED) — Steps:
   1. Analyze `git diff main...HEAD --name-only` output to determine changed components/features
   2. Check if changed files have corresponding specs in `tests/e2e/docker/` using the coverage mapping table below
   3. If gaps exist, create new spec files in `tests/e2e/docker/` following the existing pattern: numbered `NN-name.spec.ts` (next free number), `import { RustTui, resetMock, setupMock } from "./helpers/rust-tui"`, `resetMock()` in `beforeEach`, `tui.open()`, navigate to the relevant view, assert on `getScreenText()` / `keyBar()` / `selectedRow()`, and `tui.screenshot("<name>")`. Specs share one seeded DB and run sequentially, so restore any setting or state a spec changes. Seed data comes from `e2e/entrypoint.sh`; mock GitHub/Linear responses from `e2e/mock-api/` (fixtures in `e2e/mock-api/fixtures/`). Look at existing specs for reference.
   4. Clean prior artifacts: `rm -rf tests/e2e/tmp/*.png tests/e2e/tmp/test-results`
   5. Run `pnpm test:e2e` — this builds the Docker images (compiles the release binaries in the `rust-build` stage and picks up new specs via `COPY`), runs all Playwright specs, and screenshots land in `tests/e2e/tmp/`
   6. On failure: capture logs with `docker compose -f e2e/docker-compose.yml logs`, then clean up with `pnpm test:e2e:clean`
   7. On success: clean up with `pnpm test:e2e:clean`, return pass/fail summary + screenshot file list from `tests/e2e/tmp/`
   8. Upload screenshots: `bash .claude/scripts/upload-evidence.sh tests/e2e/tmp`

   ### E2E Coverage Mapping

   Use this table to determine whether existing specs already cover the changed files. Only create new specs for components/features NOT in this table.

   | Component/File | Covered By |
   |---|---|
   | `crates/am-tui/src/ui.rs` (worktree pane, row layout, key bar) | `01-dashboard.spec.ts`, plus screenshots in every spec |
   | `crates/am-tui/src/ui.rs` (`draw_help`, status legend) | `02-help.spec.ts` |
   | `crates/am-tui/src/app.rs` (navigation, selection, `w` log pane) | `01-dashboard.spec.ts` |
   | `crates/am-tui/src/app.rs` (`rebuild_rows`: repo / ticket / project headers) | `01-dashboard.spec.ts`, `05-linear.spec.ts`, `08-standalone.spec.ts` |
   | `crates/am-tui/src/settings_ui.rs` (settings editor) | `03-settings.spec.ts` |
   | `crates/am-tui/src/settings_ui.rs` (sort sub-editor) | `05-linear.spec.ts` |
   | `crates/am-tui/src/modals.rs`, `ops.rs` (new worktree form, live branch check, branch-exists choices, create) | `09-new-worktree.spec.ts` |
   | `crates/am-tui/src/modals.rs`, `ops.rs` (delete dialog, delete) | `10-delete.spec.ts` |
   | `crates/am-tui/src/jobs.rs`, jobs pane in `ui.rs`, placeholder rows, startup script (`ops::run_script`) | `09-new-worktree.spec.ts`, `10-delete.spec.ts` |
   | `crates/am-tui/src/setup.rs` (first-run wizard) | `11-setup-wizard.spec.ts` |
   | `crates/am-tui/src/data.rs` (collector, standalone "Other sessions" group) | `01-dashboard.spec.ts`, `07-subagents.spec.ts`, `08-standalone.spec.ts` |
   | `crates/am-tui/src/data.rs` (integrations thread), `crates/am-core/src/integrations/{cache,breaker}.rs`, offline hint | `06-offline-cache.spec.ts` |
   | `crates/am-tui/src/theme.rs` | Screenshots in every spec (default `ansi` theme) |
   | `crates/am-core/src/integrations/github.rs`, PR column | `04-pr-status.spec.ts` |
   | `crates/am-core/src/integrations/linear.rs`, ticket column, Linear PR fallback | `05-linear.spec.ts`, `04-pr-status.spec.ts` |
   | `crates/am-core/src/grouping.rs` (sort / filters / groups) | `05-linear.spec.ts`, `01-dashboard.spec.ts` |
   | `crates/am-core/src/status.rs`, `crates/am-hook/src/*` (hook path, subagent tracking) | `07-subagents.spec.ts`, `08-standalone.spec.ts` (seeded through `am hook-event` in `e2e/entrypoint.sh`) |
   | `crates/am-core/src/worktrees.rs` (create / delete / sync / remote probe) | `09-new-worktree.spec.ts`, `10-delete.spec.ts` |
   | `crates/am-core/src/hooks_installer.rs` | `11-setup-wizard.spec.ts` (hooks install step) |
   | `crates/am-cli/src/**` | `crates/am-cli/tests/cli.rs` (Agent A); `am repo add` / `am hook-event` / `am status` also run in `e2e/entrypoint.sh` |
   | `e2e/**`, `tests/e2e/docker/helpers/**` | All specs |

   Not covered by E2E (unit/snapshot tests only — note this in the evidence rather than adding a spec unless the diff changes visible behaviour): `crates/am-tui/src/launcher.rs` and `crates/am-core/src/launch.rs` (open IDE/terminal/URL — external apps), `crates/am-core/src/stacks.rs` (stacked-branch tree; covered by the `stacked_branches_nest_under_their_parent` snapshot), `crates/am-core/src/procscan.rs`, audio cues, and the `f` feature-request flow.

   For anything else, Agent B should create a new spec in `tests/e2e/docker/`.

4. **Aggregate evidence** — Wait for ALL sub-agents to complete. Collect all results and format into structured markdown:

   ```markdown
   ## Evidence

   ### Clippy
   <details><summary>Output</summary>

   ```
   (clippy output here)
   ```

   </details>

   ### Rust Tests (X/Y passed)
   | Crate / Test Binary | Tests | Status |
   |---------------------|-------|--------|
   | am-core (lib) | 120/120 | ✅ |
   | am-tui (lib, insta snapshots) | 40/40 | ✅ |

   <details><summary>Full test output</summary>

   ```
   (test output here)
   ```

   </details>

   ### Docker E2E Tests (X/Y passed)
   | Spec | Tests | Status |
   |------|-------|--------|
   | 01-dashboard.spec.ts | 3/3 | ✅ |
   | 04-pr-status.spec.ts | 4/4 | ✅ |

   ### Screenshots
   ![Dashboard](uploaded-url)
   ![Settings](uploaded-url)
   ```

5. **Update the PR description** (if a PR exists) — Read current body, then update the `## Evidence` section. Use the REST API to avoid the GraphQL classic projects bug:
   ```bash
   # Read current PR
   gh pr view --json number,body -q '.number,.body'
   # Update via REST API
   OWNER_REPO=$(gh repo view --json nameWithOwner -q '.nameWithOwner')
   gh api "repos/$OWNER_REPO/pulls/$PR_NUMBER" -X PATCH -f body='...'
   ```

   If no PR exists yet, return the evidence markdown for the caller to embed.

6. **Return the evidence markdown** for display.

## Completion Checklist

Before returning, verify ALL of the following:
- [ ] Clippy ran and output captured
- [ ] Agent A (Rust tests) was spawned and results collected
- [ ] Agent B (Docker E2E + screenshots) was spawned and results collected
- [ ] Evidence markdown is complete with all sections populated
- [ ] Screenshots uploaded via `upload-evidence.sh tests/e2e/tmp`
