import { test, expect } from "@playwright/test";
import { TuiPage } from "./helpers/tui-page";
import { resetMock } from "./helpers/mock-api-client";

test.beforeEach(async () => {
  await resetMock();
});

// The default linear-issue fixture links the seeded branch to ticket ENG-123.
// Project grouping was removed — worktrees show their ticket inline and are
// ordered by the sort criteria (clustering by ticket/project happens through
// the sort order, not a separate project section).
test("shows the linked Linear ticket without a project section header", async ({ page }) => {
  const tui = new TuiPage(page);
  await tui.goto();
  await tui.waitForText("main", 10_000);

  // Wait for the Linear fetch to land; the ticket identifier renders on the row.
  await tui.waitForText("ENG-123", 15_000);

  const text = await tui.getScreenText();
  // No project-section banner (═ Project ═) is rendered anymore.
  expect(text).not.toContain("═");

  await tui.screenshot("linear-ticket-no-grouping");
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

test("selection traverses worktree rows", async ({ page }) => {
  const tui = new TuiPage(page);
  await tui.goto();
  await tui.waitForText("main", 10_000);
  await tui.waitForText("ENG-123", 15_000);

  await tui.sendKey("j");
  await tui.sendKey("k");
  const text = await tui.getScreenText();
  expect(text).toContain("▸");
});
