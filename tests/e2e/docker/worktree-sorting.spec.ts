import { test, expect } from "@playwright/test";
import { TuiPage } from "./helpers/tui-page.js";
import { resetMock } from "./helpers/mock-api-client.js";

test.beforeEach(async () => {
  await resetMock();
});

// The new "Sorting & Display" settings section exposes grouping, the
// reorderable sort list, worktree filters, and per-row display toggles.
test("Settings shows the Sorting & Display section", async ({ page }) => {
  const tui = new TuiPage(page);
  await tui.goto();
  await tui.waitForText("main", 10_000);

  await tui.sendKey("s"); // open Settings
  await tui.waitForText("Sorting & Display", 10_000);

  const text = await tui.getScreenText();
  expect(text).toContain("Sort Order:");
  expect(text).toContain("Hide Merged/Closed PRs:");
  expect(text).toContain("Show PR Status:");

  await tui.screenshot("settings-sorting-display");
});

// The Sort Order field opens a dedicated full-page editor with a live example.
test("Sort Order opens the dedicated editor page with an example preview", async ({ page }) => {
  const tui = new TuiPage(page);
  await tui.goto();
  await tui.waitForText("main", 10_000);

  await tui.sendKey("s");
  await tui.waitForText("Sorting & Display", 10_000);

  // Move the selection down to the Sort Order field, then Enter to open the
  // editor. Scan for the active-row marker (▸) rather than a fixed keypress
  // count so this is robust to conditionally-shown rows (e.g. the Max Worktrees
  // value row, which becomes navigable when the worktree limit is enabled by an
  // earlier spec that shares this container's settings).
  for (let i = 0; i < 25; i++) {
    if ((await tui.getScreenText()).includes("▸ Sort Order")) break;
    await tui.sendKey("ArrowDown");
  }
  await tui.sendKey("Enter");

  await tui.waitForText("Edit Sort Order", 10_000);
  const text = await tui.getScreenText();
  expect(text).toContain("Criteria");
  expect(text).toContain("Example");
  expect(text).toContain("Grab to move");
  // example worktree rows render in the preview
  expect(text).toContain("feature/auth");

  await tui.screenshot("settings-sort-editor");
});
