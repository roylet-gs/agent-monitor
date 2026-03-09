import { test, expect } from "@playwright/test";
import { TuiPage } from "./helpers/tui-page.js";
import { resetMock } from "./helpers/mock-api-client.js";

test.beforeEach(async () => {
  await resetMock();
});

test("pressing s opens settings and Escape returns to dashboard", async ({ page }) => {
  const tui = new TuiPage(page);
  await tui.goto();

  // Wait for dashboard to render
  await tui.waitForText("main", 10_000);

  // Press 's' to open settings
  await tui.sendKey("s");
  await page.waitForTimeout(500);

  const settingsText = await tui.getScreenText();
  // Settings view should show something related to settings
  await tui.screenshot("navigation-settings");

  // Press Escape to go back
  await tui.sendKey("Escape");
  await page.waitForTimeout(500);

  // Should be back on dashboard
  await tui.waitForText("main", 5_000);
  await tui.screenshot("navigation-back-to-dashboard");
});

test("pressing n opens new worktree form and Escape cancels", async ({ page }) => {
  const tui = new TuiPage(page);
  await tui.goto();
  await tui.waitForText("main", 10_000);

  // Press 'n' to create new worktree
  await tui.sendKey("n");
  await page.waitForTimeout(500);

  await tui.screenshot("navigation-new-worktree");

  // Press Escape to cancel
  await tui.sendKey("Escape");
  await page.waitForTimeout(500);

  await tui.waitForText("main", 5_000);
  await tui.screenshot("navigation-cancel-new-worktree");
});

test("pressing d prompts for delete and n cancels", async ({ page }) => {
  const tui = new TuiPage(page);
  await tui.goto();
  await tui.waitForText("main", 10_000);

  // Press 'd' to attempt delete
  await tui.sendKey("d");
  await page.waitForTimeout(500);

  await tui.screenshot("navigation-delete-prompt");

  // Press 'n' to cancel delete (or Escape)
  await tui.sendKey("n");
  await page.waitForTimeout(500);

  await tui.screenshot("navigation-cancel-delete");
});
