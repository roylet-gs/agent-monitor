import { test, expect } from "@playwright/test";
import { TuiPage } from "./helpers/tui-page.js";
import { resetMock } from "./helpers/mock-api-client.js";

test.beforeEach(async () => {
  await resetMock();
});

// The Settings panel gains a "Limit Worktrees:" boolean toggle under the
// Worktree section (after "Hide Main Branch"), and — when enabled — a
// "Max Worktrees / Repo:" numeric row directly beneath it.
//
// worktreeLimitEnabled is FIELDS index 8:
//   openSettingsJson(0) ide(1) resumeLastSession(2) prefix(3) baseBranch(4)
//   autoSync(5) compactView(6) hideMainBranch(7) worktreeLimitEnabled(8)
const DOWN_TO_LIMIT_TOGGLE = 8;

// Branch used for the runtime-created worktree in the popup test. The entrypoint
// seeds maxWorktrees:1 (limit disabled), so a single dedicated worktree brings
// the repo to its cap once the limit is toggled on.
const CAP_BRANCH_SUFFIX = "cap-demo";

test("settings panel shows the Limit Worktrees toggle and Max Worktrees row", async ({ page }) => {
  const tui = new TuiPage(page);
  await tui.goto();
  await tui.waitForText("main", 10_000);

  // Open settings and move the selection down to the new toggle so it is in view.
  await tui.sendKey("s");
  for (let i = 0; i < DOWN_TO_LIMIT_TOGGLE; i++) {
    await tui.sendKey("ArrowDown");
  }

  await tui.waitForText("Limit Worktrees");
  await tui.screenshot("worktree-limit-setting");

  // Enable the limit — the "Max Worktrees / Repo:" row appears only when on.
  await tui.sendKey("Space");
  await tui.waitForText("Max Worktrees");
  await tui.screenshot("worktree-limit-setting-enabled");

  const text = await tui.getScreenText();
  expect(text).toContain("Limit Worktrees");
  expect(text).toContain("Max Worktrees");
});

// End-to-end: bring a repo to its cap (create one worktree with maxWorktrees:1),
// enable the limit, then attempt to create another — the bordered "Worktree
// limit reached" popup should appear and the form should NOT open.
//
// The worktree is created at runtime (not seeded in the entrypoint) because the
// E2E containers share one on-disk DB across every spec's fresh TUI process — a
// seeded worktree would shift row indices for index-sensitive specs. This spec
// runs second-to-last alphabetically (only worktree-sorting follows, and it does
// not depend on worktree rows), so the created worktree is safely contained.
test("shows the limit-reached popup when creating a worktree at the cap", async ({ page }) => {
  const tui = new TuiPage(page);
  await tui.goto();
  await tui.waitForText("main", 10_000);

  // Create a dedicated worktree so the repo reaches maxWorktrees:1 — but only if
  // one does not already exist (keeps the test idempotent across Playwright
  // retries, where the branch would already be present).
  const initial = await tui.getScreenText();
  if (!initial.includes(CAP_BRANCH_SUFFIX)) {
    await tui.sendKey("n"); // open the new-worktree form (limit still disabled)
    await tui.waitForText("New Worktree", 10_000);
    // Branch field is pre-filled with the "feature/" prefix; append the suffix.
    await tui.type(CAP_BRANCH_SUFFIX);
    await tui.sendKey("Enter"); // branch -> name field
    await tui.sendKey("Enter"); // name (empty) -> base branch field
    await tui.sendKey("Enter"); // base branch -> submit / create
    // Wait for creation to finish and the new worktree row to appear.
    await tui.waitForText(CAP_BRANCH_SUFFIX, 25_000);
  }

  // Enable the per-repo worktree limit via Settings, then Escape to save it.
  await tui.sendKey("s");
  for (let i = 0; i < DOWN_TO_LIMIT_TOGGLE; i++) {
    await tui.sendKey("ArrowDown");
  }
  await tui.waitForText("Limit Worktrees");
  await tui.sendKey("Space"); // toggle limit ON
  await tui.sendKey("Escape"); // save + return to dashboard
  await tui.waitForText("main", 10_000);
  // Let the saved settings propagate into the dashboard key-binding closure.
  await page.waitForTimeout(1500);

  // Attempt to create another worktree — the repo is at 1/1, so this is blocked.
  await tui.sendKey("n");
  await tui.waitForText("Worktree limit reached", 10_000);
  await tui.screenshot("worktree-limit-reached-popup");

  const text = await tui.getScreenText();
  expect(text).toContain("Worktree limit reached");
});
