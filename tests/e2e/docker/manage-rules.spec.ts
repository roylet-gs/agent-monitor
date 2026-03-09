import { test, expect } from "@playwright/test";
import { TuiPage } from "./helpers/tui-page.js";
import { resetMock } from "./helpers/mock-api-client.js";

test.beforeEach(async () => {
  await resetMock();
});

test("settings shows Auto-Approval Rules section with Learn from Approvals", async ({ page }) => {
  const tui = new TuiPage(page);
  await tui.goto();
  await tui.waitForText("main", 10_000);

  // Open settings
  await tui.sendKey("s");
  await page.waitForTimeout(500);

  const text = await tui.getScreenText();
  expect(text).toContain("Auto-Approval Rules");
  expect(text).toContain("Learn from Approvals");
  expect(text).toContain("Manage Rules");
  expect(text).toContain("Remove All Rules");

  await tui.screenshot("settings-auto-approval-rules");
});

test("manage rules view shows seeded rules and supports navigation", async ({ page }) => {
  const tui = new TuiPage(page);
  await tui.goto();
  await tui.waitForText("main", 10_000);

  // Open settings
  await tui.sendKey("s");
  await page.waitForTimeout(500);

  // Navigate down to "Manage Rules" — count fields to reach it
  // Open settings.json, IDE, Branch Prefix, Base Branch, Auto-sync, Compact, Hide Main,
  // Status Poll, Log Level, Max Log, GH Enabled, GH Poll, GH Include,
  // Linear Enabled, Linear Desktop, Linear API, Linear Poll, Linear Include, Linear Nickname,
  // Rules Enabled, Learn from Approvals, Manage Rules = ~21 down arrows
  for (let i = 0; i < 21; i++) {
    await tui.sendKey("ArrowDown");
  }
  await page.waitForTimeout(300);

  // Verify we're near Manage Rules (check screen text)
  let text = await tui.getScreenText();

  // If not on Manage Rules yet, try a few more
  if (!text.includes("Enter to open")) {
    for (let i = 0; i < 5; i++) {
      await tui.sendKey("ArrowDown");
      await page.waitForTimeout(100);
      text = await tui.getScreenText();
      if (text.includes("Enter to open")) break;
    }
  }

  await tui.screenshot("settings-manage-rules-selected");

  // Press Enter to open Manage Rules
  await tui.sendKey("Enter");
  await page.waitForTimeout(500);

  text = await tui.getScreenText();
  expect(text).toContain("Manage Rules");
  // Should show some of the seeded rules
  expect(text).toContain("Bash");
  // Should show rule count header
  expect(text).toMatch(/\d+ rules?\)/);

  await tui.screenshot("manage-rules-view");

  // Navigate down through rules
  await tui.sendKey("ArrowDown");
  await tui.sendKey("ArrowDown");
  await page.waitForTimeout(300);

  await tui.screenshot("manage-rules-navigated");

  // Press Escape to go back to settings
  await tui.sendKey("Escape");
  await page.waitForTimeout(500);

  text = await tui.getScreenText();
  expect(text).toContain("Settings");
  expect(text).toContain("Auto-Approval Rules");
});
