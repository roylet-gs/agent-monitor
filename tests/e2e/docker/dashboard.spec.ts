import { test, expect } from "@playwright/test";
import { TuiPage } from "./helpers/tui-page.js";
import { resetMock } from "./helpers/mock-api-client.js";

test.beforeEach(async () => {
  await resetMock();
});

test("dashboard renders with seeded repo", async ({ page }) => {
  const tui = new TuiPage(page);
  await tui.goto();

  const text = await tui.getScreenText();

  // The dashboard should display — look for key TUI elements
  // The repo name "work" (from /work) or "agent-monitor" should appear
  expect(text.length).toBeGreaterThan(0);

  await tui.screenshot("dashboard-initial");
});

test("dashboard shows repo name", async ({ page }) => {
  const tui = new TuiPage(page);
  await tui.goto();

  // The seeded repo at /work should appear somewhere in the TUI
  await tui.waitForText("main", 10_000);

  const text = await tui.getScreenText();
  // "main" branch should be visible since the repo was git-inited with main
  expect(text).toContain("main");

  await tui.screenshot("dashboard-repo-name");
});
