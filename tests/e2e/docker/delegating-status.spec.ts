import { test, expect } from "@playwright/test";
import { TuiPage } from "./helpers/tui-page.js";
import { resetMock } from "./helpers/mock-api-client.js";

test.beforeEach(async () => {
  await resetMock();
});

// The entrypoint seeds the main worktree with the "delegating" agent status
// (main turn stopped, background subagents still running). This exercises the
// new magenta pulsing-dot in the list and the "Delegating" label in the detail
// panel added by this PR.
test("dashboard shows delegating status", async ({ page }) => {
  const tui = new TuiPage(page);
  await tui.goto();
  await tui.waitForText("main", 10_000);

  // The detail panel for the selected (main) worktree should show the
  // "Delegating" status label.
  await tui.waitForText("Delegating", 10_000);

  // Let the pulsing dot settle into a rendered frame before capturing.
  await page.waitForTimeout(2000);

  const text = await tui.getScreenText();
  expect(text).toContain("Delegating");

  await tui.screenshot("delegating-status");
});
