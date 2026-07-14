import { test, expect } from "@playwright/test";
import { TuiPage } from "./helpers/tui-page.js";
import { resetMock } from "./helpers/mock-api-client.js";

test.beforeEach(async () => {
  await resetMock();
});

test("folder browser opens via settings Add Repo and Escape returns", async ({ page }) => {
  const tui = new TuiPage(page);
  await tui.goto();

  // Wait for dashboard to render
  await tui.waitForText("main", 10_000);

  // Press 's' to open settings
  await tui.sendKey("s");
  await page.waitForTimeout(500);

  // Navigate down until the "Repositories" field is active. Navigation skips
  // hidden fields (e.g. audio sounds when audio is off), so counting presses
  // against the FIELDS array is brittle.
  for (let i = 0; i < 40; i++) {
    const text = await tui.getScreenText();
    if (text.includes("▸ Repositories")) break;
    await tui.sendKey("ArrowDown");
    await page.waitForTimeout(100);
  }
  await page.waitForTimeout(300);
  expect(await tui.getScreenText()).toContain("▸ Repositories");

  // Press 'a' to add a repo, which triggers folder-browse mode
  await tui.sendKey("a");
  await page.waitForTimeout(1000);

  // The folder browser should now be visible
  await tui.waitForText("Add Repository", 5_000);

  const text = await tui.getScreenText();
  expect(text).toContain("Navigate to a git repository");

  await tui.screenshot("folder-browser");

  // Press Escape to cancel and return
  await tui.sendKey("Escape");
  await page.waitForTimeout(500);

  // Should return to dashboard (since repos still exist)
  await tui.waitForText("main", 5_000);
  await tui.screenshot("folder-browser-back-to-dashboard");
});
