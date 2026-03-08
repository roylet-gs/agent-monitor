# Capture E2E Evidence

Collect build, test, and visual evidence that the current changes work. Uses isolated data dirs and sub-agents for parallel execution.

**IMPORTANT: You MUST complete ALL steps below. Do NOT skip steps, cut corners, or summarize. Every agent MUST be spawned. Every result MUST be collected.**

## Steps

1. **Build check** — Run `pnpm build` and capture the output. If it fails, stop and report.

2. **Analyze diff** — Run `git diff main...HEAD --name-only` to identify changed files. Map them to relevant test scenarios:
   - Changed `src/lib/*.ts` → relevant unit tests in `tests/lib/`
   - Changed `src/commands/**/*.ts` → E2E: test the corresponding CLI commands
   - Changed `src/components/*.tsx` or `src/hooks/*.ts` → TUI screenshot needed
   - Changed `src/cli.tsx` → E2E: test basic CLI invocation

3. **Spawn ALL parallel sub-agents** using the Agent tool. You MUST launch all applicable agents in a SINGLE message with PARALLEL tool calls. Do NOT run them sequentially. Do NOT skip any agent.

   **Agent A: Unit tests** (REQUIRED) — Run `pnpm test` (vitest) with relevant test files. Capture per-test pass/fail. Return structured results as a table.

   **Agent B: E2E/CLI tests** (REQUIRED) — Create an isolated data dir via:
   ```bash
   AM_DATA_DIR=$(mktemp -d /tmp/am-evidence-XXXXXX)
   ```
   Then run actual CLI commands to verify they work end-to-end. For example:
   ```bash
   AM_DATA_DIR=$dir npx tsx src/cli.tsx repo add .
   AM_DATA_DIR=$dir npx tsx src/cli.tsx repo list
   AM_DATA_DIR=$dir npx tsx src/cli.tsx worktree list
   ```
   Capture command output as evidence. Clean up the temp dir when done.

   **Agent C: TUI screenshots** (REQUIRED if UI files changed, otherwise note why skipped) —
   a. Source the seed script: `source .claude/scripts/seed-evidence-data.sh`
   b. Start ttyd: `bash .claude/scripts/capture-tui.sh "npx tsx src/cli.tsx" .github/evidence 7681`
   c. Wait 3 seconds for the app to render
   d. Use headless Playwright MCP tools (`mcp__playwright__*`) to navigate, wait, and screenshot
   e. Save screenshots to `.github/evidence/`
   f. Clean up: `bash .claude/scripts/cleanup-tui.sh`

   If ttyd is NOT installed, skip screenshots and note it in the evidence.

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
   | Test | Status |
   |------|--------|
   | test name here | ✅ |

   <details><summary>Full test output</summary>

   ```
   (test output here)
   ```

   </details>

   ### E2E Tests
   | Scenario | Status | Details |
   |----------|--------|---------|
   | `am repo add .` registers repo | ✅ | Output: "Added repository agent-monitor" |
   | `am repo list` shows registered repo | ✅ | Output shows 1 repo |

   ### Screenshots
   ![Dashboard](url)
   ```

5. **Upload screenshots** (if any) — Run `bash .claude/scripts/upload-evidence.sh` to push images to the `evidence-images` orphan branch and get permanent URLs.

6. **Update the PR description** (if a PR exists) — Read current body, then update the `## Evidence` section. Use the REST API to avoid the GraphQL classic projects bug:
   ```bash
   # Read current PR
   gh pr view --json number,body -q '.number,.body'
   # Update via REST API
   OWNER_REPO=$(gh repo view --json nameWithOwner -q '.nameWithOwner')
   gh api "repos/$OWNER_REPO/pulls/$PR_NUMBER" -X PATCH -f body='...'
   ```

   If no PR exists yet, return the evidence markdown for the caller to embed.

7. **Return the evidence markdown** for display.

## Completion Checklist

Before returning, verify ALL of the following:
- [ ] Build ran and output captured
- [ ] Agent A (unit tests) was spawned and results collected
- [ ] Agent B (E2E/CLI tests) was spawned and results collected
- [ ] Agent C (TUI screenshots) was spawned if UI files changed, OR explicitly noted why it was skipped
- [ ] Evidence markdown is complete with all sections populated
- [ ] Screenshots uploaded if any were taken
