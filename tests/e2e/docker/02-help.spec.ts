import { test, expect } from "@playwright/test";
import { RustTui, resetMock } from "./helpers/rust-tui";

test.beforeEach(async () => {
  await resetMock();
});

test("? opens the key help overlay and closes it again", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();

  await tui.sendKey("?");
  await tui.waitForText(" Keys ");
  const text = await tui.getScreenText();
  expect(text).toContain("toggle help");
  expect(text).toContain("new / delete worktree / focus jobs pane");
  expect(text).toContain("open PR / Linear ticket / Linear project");
  expect(text).toContain("top / bottom");
  expect(text).toContain("refresh now (bypasses backoff)");
  // View logs (L) shares a line with the log-pane toggle.
  expect(text).toMatch(/w \/ L\s+toggle log pane \/ open the log file \(or folder\)/);
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

  await tui.sendKey("?");
  await tui.waitForGone(" Keys ");
});
