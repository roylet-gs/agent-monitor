# Capture E2E Evidence

Collect lint, test, and visual evidence that the current changes work. Uses the Cargo test suite plus the Docker-based E2E infrastructure (real `am` / `am-tui` / `am-hook` binaries behind ttyd, driven by Playwright) for screenshots and integration tests.

**IMPORTANT: You MUST complete ALL steps below. Do NOT skip steps, cut corners, or summarize. Every agent MUST be spawned. Every result MUST be collected.**

## Steps

1. **Lint check** — Run `cargo clippy --workspace --all-targets -- -D warnings` and capture the output. If it fails, stop and report.

2. **Analyze diff** — Run `bash e2e/affected-specs.sh --explain` (stderr: each changed file → the specs that cover it, `ALL`, or `no E2E`; stdout: the specs that will run). It is the file → spec mapping, so use it to decide whether new E2E specs are needed.

3. **Spawn ALL parallel sub-agents** using the Agent tool. You MUST launch all applicable agents in a SINGLE message with PARALLEL tool calls. Do NOT run them sequentially. Do NOT skip any agent.

   **Agent A: Rust tests** (REQUIRED) — Run `cargo test --workspace`. Capture per-crate / per-test-binary pass/fail counts (am-core, am-hook incl. `tests/hook.rs`, am-tui incl. insta snapshots, am-cli incl. `tests/cli.rs`) and list any failing tests by name. If an insta snapshot fails, include the snapshot diff; do NOT accept snapshots (`INSTA_UPDATE=always`) unless the UI change is intended by the diff, and say so if you did. Return structured results as a table.

   **Agent B: Docker E2E + screenshots** (REQUIRED) — Steps:
   1. Run `bash e2e/affected-specs.sh --explain` to see which specs cover each changed file. The mapping lives in that script's `case` table; files it doesn't list map to `ALL`.
   2. Decide on gaps: the diff changes visible behaviour that no mapped spec asserts (read the mapped specs), or `--explain` reports a new source file falling through to `ALL`.
   3. If gaps exist, create new spec files in `tests/e2e/docker/` following the existing pattern: numbered `NN-name.spec.ts` (next free number), `import { RustTui, resetMock, setupMock } from "./helpers/rust-tui"`, `resetMock()` in `beforeEach`, `tui.open()`, navigate to the relevant view, assert on `getScreenText()` / `keyBar()` / `selectedRow()`, and `tui.screenshot("<name>")`. Specs share one seeded DB and run sequentially (and may run on their own, so never rely on an earlier spec's leftovers); restore any setting or state a spec changes. Seed data comes from `e2e/entrypoint.sh`; mock GitHub/Linear responses from `e2e/mock-api/` (fixtures in `e2e/mock-api/fixtures/`). Look at existing specs for reference. Add the new spec's number to the rows of `e2e/affected-specs.sh` for the code it covers (a changed spec file always runs itself).
   4. Clean prior artifacts: `rm -rf tests/e2e/tmp/*.png tests/e2e/tmp/test-results`
   5. Run `pnpm test:e2e`. It runs only the specs `affected-specs.sh` picks. If none are affected it exits 0 without building: report "no E2E-relevant changes" and skip the remaining steps. Otherwise it builds the Docker images (compiles the release binaries in the `rust-build` stage and picks up new specs via `COPY`), runs the selected specs, prints `E2E timings: build …s, specs …s`, and screenshots land in `tests/e2e/tmp/`. Use `pnpm test:e2e:all` only when asked for a full run.
   6. On failure: capture logs with `bash e2e/run.sh logs` (the stack is per-worktree, see `e2e/run.sh`), then clean up with `pnpm test:e2e:clean`
   7. On success: clean up with `pnpm test:e2e:clean`, return pass/fail summary (which specs ran, which were skipped as unaffected) + screenshot file list from `tests/e2e/tmp/`
   8. Upload screenshots: `bash .claude/scripts/upload-evidence.sh tests/e2e/tmp`

   Not covered by E2E (unit/snapshot tests only; `affected-specs.sh` maps them to `no E2E`, so note this in the evidence rather than adding a spec unless the diff changes visible behaviour): `crates/am-tui/src/launcher.rs` and `crates/am-core/src/launch.rs` (open IDE/terminal/URL — external apps), `crates/am-core/src/stacks.rs` (stacked-branch tree; covered by the `stacked_branches_nest_under_their_parent` snapshot), `crates/am-core/src/procscan.rs`, most of `crates/am-cli` (`crates/am-cli/tests/cli.rs`, Agent A), audio cues, and the `f` feature-request flow.

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
   Specs run: (from `affected-specs.sh`; list skipped specs as unaffected)
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
