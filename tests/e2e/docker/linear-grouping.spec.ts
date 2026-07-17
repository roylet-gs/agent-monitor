import { test, expect } from "@playwright/test";
import { TuiPage } from "./helpers/tui-page.js";
import { resetMock } from "./helpers/mock-api-client.js";

test.beforeEach(async () => {
  await resetMock();
});

// The default linear-issue fixture links every branch to ticket ENG-123 in
// project "Dashboard Revamp", so the dashboard groups the seeded worktree
// under a project header.
test("groups worktrees under a Linear project header", async ({ page }) => {
  const tui = new TuiPage(page);
  await tui.goto();
  await tui.waitForText("main", 10_000);

  // Wait for the initial Linear fetch to land and the project header to render
  await tui.waitForText("Dashboard Revamp", 15_000);

  const text = await tui.getScreenText();
  // Project header renders above the worktree row
  expect(text.indexOf("Dashboard Revamp")).toBeLessThan(text.indexOf("ENG-123"));

  await tui.screenshot("linear-project-grouping");
});

test("shows the [p]roject action when the selected worktree has a Linear project", async ({ page }) => {
  const tui = new TuiPage(page);
  await tui.goto();
  await tui.waitForText("main", 10_000);
  // Wait for the Linear fetch so the selected worktree carries its project.
  await tui.waitForText("Dashboard Revamp", 15_000);

  // The action bar exposes [p]roject once a project URL is available.
  await tui.waitForText("[p]roject", 5_000);

  await tui.screenshot("linear-project-action");
});

test("selection still traverses worktree rows with project headers present", async ({ page }) => {
  const tui = new TuiPage(page);
  await tui.goto();
  await tui.waitForText("main", 10_000);
  await tui.waitForText("Dashboard Revamp", 15_000);

  // j/k moves the ▸ marker across rows only; headers are not selectable.
  await tui.sendKey("j");
  await tui.sendKey("k");
  const text = await tui.getScreenText();
  expect(text).toContain("▸");

  await tui.screenshot("linear-project-grouping-selection");
});
