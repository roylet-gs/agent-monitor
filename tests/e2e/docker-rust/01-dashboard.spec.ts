import { test, expect } from "@playwright/test";
import { RustTui, resetMock } from "./helpers/rust-tui";

test.beforeEach(async () => {
  await resetMock();
});

test("dashboard renders both panes with the seeded repo", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  await tui.waitForText("feature/to-delete");

  const text = await tui.getScreenText();
  expect(text).toContain("[1] Worktrees");
  expect(text).toContain("[2] Subagents");
  // Repo header (two groups: the repo + "Other sessions").
  expect(text).toMatch(/│work\s/);
  expect(text).toContain("main");
  expect(await tui.statusBar()).toContain(" WORKTREES ");
  expect(await tui.statusBar()).toContain("? help");

  await tui.screenshot("rust-dashboard");
});

test("j/k move the selection and tab/1/2 switch the focused pane", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();

  await tui.selectRow("feature/to-delete");
  await tui.sendKey("j");
  expect((await tui.contextLine()).trim()).toMatch(/^main/);
  await tui.sendKey("k");
  expect((await tui.contextLine()).trim()).toMatch(/^feature\/to-delete/);

  await tui.sendKey("Tab");
  expect(await tui.statusBar()).toContain(" SUBAGENTS ");
  await tui.sendKey("1");
  expect(await tui.statusBar()).toContain(" WORKTREES ");
  await tui.sendKey("2");
  expect(await tui.statusBar()).toContain(" SUBAGENTS ");
  // Esc from the Subagents pane returns focus instead of quitting.
  await tui.sendKey("Escape");
  expect(await tui.statusBar()).toContain(" WORKTREES ");
});

test("w toggles the log pane", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  await tui.sendKey("w");
  await tui.waitForText("[w] Log");
  await tui.sendKey("w");
  await tui.waitForGone("[w] Log");
});
