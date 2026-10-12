import { test, expect } from "@playwright/test";
import { RustTui, resetMock } from "./helpers/rust-tui";

// e2e/entrypoint.sh writes a transcript for main's session under
// $HOME/.claude/projects/-work/ whose last assistant turn used 150k tokens:
// 75% of the default 200k window, so the bar is 6 of 8 cells and yellow.

test.beforeEach(async () => {
  await resetMock();
});

test("context-window bar sits at the far right of the agent's row", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  await tui.waitForMatch(/ main\b.*██████░░ 150k │/);

  // Rows without a session keep the column blank (and aligned).
  await tui.selectRow("feature/to-delete");
  const row = await tui.selectedRow();
  expect(row).not.toMatch(/░|\d+k │/);

  await tui.selectRow("main");
  expect(await tui.selectedRow()).toMatch(/150k │/);
  await tui.screenshot("context-window");
});
