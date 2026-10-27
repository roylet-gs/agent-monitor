import { test, expect } from "@playwright/test";
import { RustTui, resetMock } from "./helpers/rust-tui";

test.beforeEach(async () => {
  await resetMock();
});

test("h opens the key help overlay and closes it again", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();

  await tui.sendKey("h");
  await tui.waitForText(" Keys ");
  const text = await tui.getScreenText();
  expect(text).toContain("toggle help");
  expect(text).toContain("new / delete worktree / focus jobs pane");
  expect(text).toContain("open PR / Linear ticket / Linear project");
  expect(text).toContain("top / bottom");
  expect(text).toContain("refresh now (bypasses backoff)");
  // Viewing the log file moved to Settings.
  expect(text).toMatch(/w\s+toggle log pane \(Settings → View logs… opens the file\)/);
  expect(text).toMatch(/h\s+toggle help/);
  // The review page: full, beside the list, or in a new window.
  expect(text).toMatch(/v \/ V \/ ctrl-v\s+PRs to review: page \/ beside the list \/ new window/);
  // Status legend.
  for (const s of ["executing", "planning", "waiting", "delegating", "done", "idle"]) {
    expect(text).toContain(s);
  }
  // PR review-state legend.
  expect(text).toContain("PR approved=CI ✓  approved=CI ✗/◌");
  // The `t` terminal and `c` agent actions are gone.
  expect(text).not.toContain("terminal at worktree");
  expect(text).not.toContain("terminal resuming the agent");
  await tui.screenshot("help");
  await tui.screenshot("help-pr-legend");
  await tui.screenshot("help-overlay");
  await tui.screenshot("help-view-logs");

  await tui.sendKey("h");
  await tui.waitForGone(" Keys ");
});
