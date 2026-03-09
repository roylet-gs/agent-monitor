# Capture E2E Evidence

Collect build, test, and visual evidence that the current changes work. Uses Docker-based E2E infrastructure for screenshots and integration tests.

**IMPORTANT: You MUST complete ALL steps below. Do NOT skip steps, cut corners, or summarize. Every agent MUST be spawned. Every result MUST be collected.**

## Steps

1. **Build check** — Run `pnpm build` and capture the output. If it fails, stop and report.

2. **Analyze diff** — Run `git diff main...HEAD --name-only` to identify changed files. Use the coverage mapping table below to determine if new E2E specs are needed.

3. **Spawn ALL parallel sub-agents** using the Agent tool. You MUST launch all applicable agents in a SINGLE message with PARALLEL tool calls. Do NOT run them sequentially. Do NOT skip any agent.

   **Agent A: Unit tests** (REQUIRED) — Run `pnpm test` (vitest) with relevant test files. Capture per-test pass/fail. Return structured results as a table.

   **Agent B: Docker E2E + screenshots** (REQUIRED) — Steps:
   1. Analyze `git diff main...HEAD --name-only` output to determine changed components/features
   2. Check if changed components have corresponding specs in `tests/e2e/docker/` using the coverage mapping table below
   3. If gaps exist, create new spec files in `tests/e2e/docker/` following the existing pattern (import `TuiPage` from `./pages/tui-page`, navigate to relevant view, take screenshots). Look at existing specs for reference.
   4. Clean prior artifacts: `rm -rf tests/e2e/tmp/*.png tests/e2e/tmp/test-results`
   5. Run `pnpm test:e2e` — this builds Docker images (picks up new specs via `COPY`), runs all Playwright specs, screenshots land in `tests/e2e/tmp/`
   6. On failure: capture logs with `docker compose -f e2e/docker-compose.yml logs`, then clean up with `pnpm test:e2e:clean`
   7. On success: clean up with `pnpm test:e2e:clean`, return pass/fail summary + screenshot file list from `tests/e2e/tmp/`
   8. Upload screenshots: `bash .claude/scripts/upload-evidence.sh tests/e2e/tmp`

   ### E2E Coverage Mapping

   Use this table to determine whether existing specs already cover the changed files. Only create new specs for components/features NOT in this table.

   | Component/File | Covered By |
   |---|---|
   | `src/components/Dashboard.tsx`, `WorktreeList.tsx`, `StatusBar.tsx`, `ActionBar.tsx` | `dashboard.spec.ts`, `screenshots.spec.ts` |
   | `src/components/SettingsPanel.tsx` | `navigation.spec.ts` (s key), `screenshots.spec.ts` |
   | `src/components/NewWorktreeForm.tsx` | `navigation.spec.ts` (n key) |
   | `src/components/DeleteConfirm.tsx` | `navigation.spec.ts` (d key) |
   | `src/components/WorktreeDetail.tsx` | `dashboard.spec.ts` |
   | PR-related changes (`src/lib/github.ts`) | `pr-status.spec.ts` |
   | Linear-related changes (`src/lib/linear.ts`) | `pr-status.spec.ts` (via mock-api) |
   | Setup wizard (`src/components/SetupWizard.tsx`) | `setup-wizard.spec.ts` |

   For anything not in this table, Agent B should create a new spec in `tests/e2e/docker/`.

4. **Aggregate evidence** — Wait for ALL sub-agents to complete. Collect all results and format into structured markdown:

   ```markdown
   ## Evidence

   ### Build
   <details><summary>Output</summary>

   ```
   (build output here)
   ```

   </details>

   ### Unit Tests (X/Y passed)
   | Test File | Tests | Status |
   |-----------|-------|--------|
   | test name here | 5/5 | ✅ |

   <details><summary>Full test output</summary>

   ```
   (test output here)
   ```

   </details>

   ### Docker E2E Tests (X/Y passed)
   | Spec | Tests | Status |
   |------|-------|--------|
   | dashboard.spec.ts | 2/2 | ✅ |
   | navigation.spec.ts | 3/3 | ✅ |

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
- [ ] Build ran and output captured
- [ ] Agent A (unit tests) was spawned and results collected
- [ ] Agent B (Docker E2E + screenshots) was spawned and results collected
- [ ] Evidence markdown is complete with all sections populated
- [ ] Screenshots uploaded via `upload-evidence.sh tests/e2e/tmp`
