import { test, expect } from "@playwright/test";
import { RustTui, resetMock } from "./helpers/rust-tui";

test.beforeEach(async () => {
  await resetMock();
});

const MOUSE = /Mouse\s+(on|off)/;

async function mouseValue(tui: RustTui): Promise<string> {
  const m = MOUSE.exec(await tui.getScreenText());
  if (!m) throw new Error(`Mouse row not found:\n${await tui.getScreenText()}`);
  return m[1];
}

test("click selects a row, the wheel moves the selection", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  await tui.selectRow("main");

  await tui.clickText("feature/to-delete");
  expect(await tui.selectedName()).toBe("feature/to-delete");
  await tui.screenshot("mouse-click-select");

  await tui.sendKey("Home");
  const first = await tui.selectedName();
  await tui.wheel(1);
  expect(await tui.selectedName()).not.toBe(first);
  await tui.wheel(-5);
  expect(await tui.selectedName()).toBe(first);
});

test("key-bar hints are clickable and a click closes help", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();

  await tui.clickText("? help");
  await tui.waitForText("toggle help");
  await tui.screenshot("mouse-keybar-help");
  await tui.clickText("─Worktrees");
  await tui.waitForGone("toggle help", 5_000);
});

test("hovering highlights rows and key-bar hints", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  await tui.selectRow("main");

  const row = await tui.hoverText("feature/to-delete");
  await expect.poll(() => tui.bgIsDefault(row.col, row.row)).toBe(false);
  await tui.screenshot("mouse-hover-row");
  // Hovering a hint lights it up and the row goes back to normal.
  const hint = await tui.hoverText("? help");
  await expect.poll(() => tui.bgIsDefault(hint.col, hint.row)).toBe(false);
  expect(await tui.bgIsDefault(row.col, row.row)).toBe(true);
  // Hover never moves the selection.
  expect(await tui.selectedName()).toBe("main");
});

test("settings pages and dialogs take clicks and hover", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  await tui.clickText("s settings");
  await tui.waitForText("esc close");

  // First click selects the row, the second opens the page (like ⏎).
  await tui.clickText("Key bindings…");
  await tui.clickText("Key bindings…");
  await tui.waitForText("Key bindings —");
  const cell = await tui.hoverText("Refresh");
  await expect.poll(() => tui.bgIsDefault(cell.col + 2, cell.row)).toBe(false);
  await tui.screenshot("mouse-hover-key-bindings");
  // Header hints are buttons.
  await tui.clickText("esc back");
  await tui.waitForGone("Key bindings —", 5_000);
  await tui.clickText("esc close");
  await tui.waitForGone("esc close", 5_000);

  // Dialog choices are buttons too.
  await tui.selectRow("feature/to-delete");
  await tui.clickText("d delete");
  await tui.waitForText("Cancel");
  await tui.hoverText("Cancel");
  await tui.screenshot("mouse-hover-delete-dialog");
  await tui.clickText("Cancel");
  await tui.waitForGone("Remove this worktree from disk?", 5_000);
  expect(await tui.selectedName()).toBe("feature/to-delete");
});

test("double-click opens the worktree", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  await tui.selectRow("main");
  const name = await tui.selectedName();
  await tui.clickText(`${name} `, { double: true });
  // Opening has no editor in the container; any launcher feedback (popup or
  // notice) proves Enter fired. The row stays selected either way.
  expect(await tui.selectedName()).toBe(name);
  await tui.screenshot("mouse-double-click");
});

test("Mouse can be turned off in settings by clicking it", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  await tui.clickText("s settings");
  await tui.waitForText("esc close");
  expect(await mouseValue(tui)).toBe("on");

  await tui.clickText("Mouse  "); // select the field
  await tui.waitForText("off restores the terminal's own text selection");
  await tui.clickText("Mouse  "); // change it
  expect(await mouseValue(tui)).toBe("off");
  await tui.screenshot("mouse-setting-off");
  await tui.sendKey("Escape");
  await tui.waitForGone("esc close");

  // Clicks no longer reach the app.
  await tui.selectRow("main");
  await tui.clickText("? help");
  await page.waitForTimeout(500);
  expect(await tui.getScreenText()).not.toContain("toggle help");
  expect(await tui.selectedName()).toBe("main");

  // Restore (keyboard: the mouse is off).
  await tui.sendKey("s");
  await tui.waitForText("esc close");
  // Open in → Editor command → Resume last session → Repositories… → Harness plugins… → Key bindings… → Mouse.
  await tui.press("j", 6);
  await tui.waitForText("off restores the terminal's own text selection");
  await tui.sendKey(" ");
  expect(await mouseValue(tui)).toBe("on");
  await tui.sendKey("Escape");
});
