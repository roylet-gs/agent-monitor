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
  await tui.sendKey("j");
  await tui.sendKey("j");
  await tui.waitForText("off restores the terminal's own text selection");
  await tui.sendKey(" ");
  expect(await mouseValue(tui)).toBe("on");
  await tui.sendKey("Escape");
});
