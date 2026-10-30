import { test, expect } from "@playwright/test";
import { RustTui, resetMock } from "./helpers/rust-tui";

// Pulling into the tmp space (Reviewing) happens only from the review page
// (28-review-requests): `t` on the dashboard does nothing, and the New
// Worktree form has no tmp space switch.

test.beforeEach(async () => {
  await resetMock();
});

test("t on the dashboard does nothing", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  // By name: the row's PR and age columns may still be filling in.
  const before = await tui.selectedName();

  expect(await tui.keyBar()).not.toContain("pull");
  await tui.sendKey("t");
  await tui.waitForText("· Worktrees");
  const screen = await tui.getScreenText();
  expect(screen).not.toContain("Pull into Reviewing");
  expect(screen).not.toContain("─Jobs");
  expect(await tui.selectedName()).toBe(before);

  // The New Worktree form has no tmp space switch either.
  await tui.sendKey("n");
  await tui.waitForText("New worktree");
  expect(await tui.getScreenText()).not.toContain("Reviewing ‹");
  await tui.sendKey("Escape");
  await tui.waitForGone("New worktree", 5_000);
});
