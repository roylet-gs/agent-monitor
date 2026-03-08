# Capture E2E Evidence

Collect visual and text evidence that the current changes work, for inclusion in a PR.

## Steps

1. **Build check** -- Run `pnpm build` and capture the output. If it fails, stop and report.

2. **Test check** -- Run `pnpm test` (or `pnpm vitest run`) and capture the output. If it fails, stop and report.

3. **TUI screenshot** (if ttyd is available):
   a. Start ttyd: `bash .claude/scripts/capture-tui.sh "pnpm start"`
   b. Wait 3 seconds for the app to render
   c. Use `mcp__playwright__browser_navigate` to go to the ttyd URL (printed by the script)
   d. Use `mcp__playwright__browser_wait_for` to wait for the terminal to fully render (look for text content)
   e. Use `mcp__playwright__browser_take_screenshot` to capture the initial state
   f. Save screenshot to `.github/evidence/` with a descriptive name (e.g., `dashboard.png`)
   g. Interact with the TUI if needed using `mcp__playwright__browser_press_key` or `mcp__playwright__browser_type`
   h. Take additional screenshots of different states as needed
   i. Clean up: `bash .claude/scripts/cleanup-tui.sh`

   If ttyd is NOT installed, skip screenshots and note it in the evidence. Build + test output is still valuable.

4. **Compile evidence into markdown** for the PR body:
   ```markdown
   ## Evidence

   ### Build
   <details><summary>Output</summary>

   ```
   (build output here)
   ```

   </details>

   ### Tests
   <details><summary>Output (X passed)</summary>

   ```
   (test output here)
   ```

   </details>

   ### Screenshots
   ![Dashboard](/.github/evidence/dashboard.png)
   ![Detail View](/.github/evidence/detail.png)
   ```

5. **Commit evidence** -- Stage and commit `.github/evidence/` files to the current branch.

6. **Return the markdown** so the caller (e.g., `/create-pr`) can embed it in the PR body.
