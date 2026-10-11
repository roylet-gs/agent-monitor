# Create or Update PR

Idempotent: detects existing PRs and updates them instead of failing.

## Steps

1. **Lint & test** — Run `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`. Stop if either fails.

2. **Ensure test coverage** — Check that critical paths affected by the diff have tests (unit tests next to the code in `crates/*/src`, integration tests in `crates/am-hook/tests/` and `crates/am-cli/tests/`, insta snapshots for `crates/am-tui/src/ui.rs`). If tests need to be added, add them and re-test before continuing. If a UI change is intended, accept snapshots with `INSTA_UPDATE=always cargo test -p am-tui` and review the `.snap` diffs.

3. **Detect existing PR** — Run:
   ```bash
   gh pr view --json number,title,body,url 2>/dev/null
   ```
   If a PR exists:
   - Save the PR number, current title, body, and URL
   - Note what evidence already exists in the body
   - Will update (not recreate) the PR later

4. **Analyze diff for test scenarios** — Run `git diff main...HEAD --name-only` to identify all changes. Determine:
   - Which crates and test modules are relevant
   - Which E2E scenarios should be tested (CLI commands, feature flows)
   - Whether TUI screenshots are needed (if `crates/am-tui` or anything shown on the dashboard changed)
   - Whether docs need updating (`CLAUDE.md`, `README.md`, `STATUS_MAPPING.md` for status-rule changes)

5. **Run `/capture-evidence`** — This spawns sub-agents to collect per-test evidence based on the diff analysis. The evidence markdown is returned.

6. **Commit and push** — Stage any new/changed files and push:
   ```bash
   git add -A
   git commit -m "chore: add test evidence"  # only if there are changes
   git push -u origin HEAD
   ```

7. **Create or update PR**:

   **If no PR exists:**
   ```bash
   gh pr create --title "feat: ..." --body "$(cat <<'EOF'
   ## Summary
   ...

   ## Evidence
   (evidence markdown from step 5)

   ## Test Plan
   ...
   EOF
   )"
   ```

   **If PR exists — update via REST API** (avoids `gh pr edit` GraphQL classic projects bug):
   ```bash
   OWNER_REPO=$(gh repo view --json nameWithOwner -q '.nameWithOwner')
   PR_NUMBER=$(gh pr view --json number -q '.number')
   ```
   Read the current body, replace/append the `## Evidence` section with fresh evidence, then:
   ```bash
   gh api "repos/$OWNER_REPO/pulls/$PR_NUMBER" -X PATCH -f body='...'
   ```

   If screenshots exist, upload them first via `bash .claude/scripts/upload-evidence.sh tests/e2e/tmp`.

8. **Wait for checks** — Monitor CI status:
   ```bash
   gh pr checks --watch
   ```
   Report final status. If checks fail, investigate and report what failed.

9. **Return the PR URL**.
