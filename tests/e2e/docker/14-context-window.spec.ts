import { test, expect } from "@playwright/test";
import { RustTui, resetMock } from "./helpers/rust-tui";

// e2e/entrypoint.sh writes a transcript for main's session under
// $HOME/.claude/projects/-work/ whose last assistant turn used 150k tokens:
// 15% of the default 1M window, so the bar is a cell and a bit, green.

test.beforeEach(async () => {
  await resetMock();
});

const CTX_HELP = "a session past it doubles the scale until it fits";

/** Open settings, move to "Context window (tokens)" and type `value` into it. */
async function setContextWindow(tui: RustTui, value: string): Promise<void> {
  await tui.sendKey("s");
  await tui.waitForText("esc close");
  for (let i = 0; i < 40 && !(await tui.getScreenText()).includes(CTX_HELP); i++) {
    await tui.sendKey("j");
  }
  await tui.waitForText(CTX_HELP);
  await tui.sendKey("Enter");
  await tui.press("Backspace", 8);
  await tui.type(value);
  await tui.sendKey("Enter");
  await tui.sendKey("Escape");
  await tui.waitForGone("esc close");
}

test("context-window bar sits at the far right of the agent's row", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  await tui.waitForMatch(/ main\b.*█▎░░░░░░ 150k │/);

  // Rows without a session keep the column blank (and aligned).
  await tui.selectRow("feature/to-delete");
  const row = await tui.selectedRow();
  expect(row).not.toMatch(/░|\d+k │/);

  await tui.selectRow("main");
  expect(await tui.selectedRow()).toMatch(/150k │/);
  await tui.screenshot("context-window");
});

test("a session past the configured window doubles the scale until it fits", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();

  // 150k is past 100k, so the bar measures against 200k: 75%, 6 of 8 cells.
  await setContextWindow(tui, "100k");
  await tui.waitForMatch(/ main\b.*██████░░ 150k │/);
  await tui.screenshot("context-window-doubled");

  // Restore the default.
  await setContextWindow(tui, "1m");
  await tui.waitForMatch(/ main\b.*█▎░░░░░░ 150k │/);
});
