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
  expect(text).toContain("new worktree / delete worktree");
  expect(text).toContain("subagents: this worktree ↔ all");
  expect(text).toContain("refresh now (bypasses backoff)");
  // Status legend.
  for (const s of ["executing", "planning", "waiting", "delegating", "done", "idle"]) {
    expect(text).toContain(s);
  }
  await tui.screenshot("rust-help");

  await tui.sendKey("?");
  await tui.waitForGone(" Keys ");
});
