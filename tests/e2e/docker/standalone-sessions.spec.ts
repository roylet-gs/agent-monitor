import { test, expect } from "@playwright/test";
import { TuiPage } from "./helpers/tui-page.js";
import { resetMock } from "./helpers/mock-api-client.js";

test.beforeEach(async () => {
  await resetMock();
});

test("dashboard shows Other Sessions section with seeded standalone sessions", async ({ page }) => {
  const tui = new TuiPage(page);
  await tui.goto();

  // Wait for the standalone sessions section to render
  await tui.waitForText("Other Sessions", 10_000);

  const text = await tui.getScreenText();
  expect(text).toContain("Other Sessions");
  // The abbreviated path should show the last segments
  expect(text).toContain("my-project");
  expect(text).toContain("scripts");

  await tui.screenshot("standalone-sessions-list");
});

test("standalone session detail shows path and status", async ({ page }) => {
  const tui = new TuiPage(page);
  await tui.goto();

  await tui.waitForText("Other Sessions", 10_000);

  // Navigate down to the first standalone session (after the worktree entries)
  // The seeded repo has 1 worktree (main), so press down once to reach first standalone
  await tui.sendKey("j");

  const text = await tui.getScreenText();
  // The detail panel should show the full path for the selected standalone session
  expect(text).toContain("/home/user/my-project");

  await tui.screenshot("standalone-session-detail");
});
