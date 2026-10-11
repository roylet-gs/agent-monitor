import { test, expect } from "@playwright/test";
import { RustTui, resetMock } from "./helpers/rust-tui";

test.beforeEach(async () => {
  await resetMock();
});

test("delete dialog for a worktree can be cancelled", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  await tui.selectRow("feature/to-delete");

  await tui.sendKey("d");
  await tui.waitForText('Delete worktree "feature/to-delete"');
  await tui.waitForText("Remove this worktree from disk?");
  const text = await tui.getScreenText();
  expect(text).toMatch(/⏎ \/ y\s+Yes/);
  expect(text).toMatch(/n \/ esc\s+Cancel/);
  await tui.screenshot("rust-delete-dialog");

  await tui.sendKey("Escape");
  await tui.waitForGone("Remove this worktree from disk?");
  await tui.waitForText("feature/to-delete");
});

test("the main checkout can't be deleted", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  await tui.selectRow("main");
  await tui.sendKey("d");
  await tui.waitForText("The main checkout can't be deleted", 3_000);
  expect(await tui.getScreenText()).not.toContain("Remove this worktree from disk?");
});

test("create then delete a worktree end to end (incl. its local branch)", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  const branch = await tui.createWorktree(`e2e-del-${Date.now() % 1_000_000}`);

  await tui.selectRow(branch);
  await tui.sendKey("d");
  await tui.waitForText(`Delete worktree "${branch}"`);
  await tui.sendKey("y");
  await tui.waitForText(`Also delete local branch ${branch}?`);
  await tui.sendKey("y");
  // Gone from the list, the "Deleting …" modal and the "Deleted …" notice.
  await tui.waitForGone(branch, 20_000);
  await tui.screenshot("rust-worktree-deleted");
});
