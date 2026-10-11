import { test, expect } from "@playwright/test";
import { RustTui, resetMock } from "./helpers/rust-tui";

// Seeded by `am-rs hook-event --worktree /home/user/my-project` (an untracked
// directory → standalone session) in e2e/entrypoint-rust.sh.

test.beforeEach(async () => {
  await resetMock();
});

test("standalone sessions are listed under Other sessions and selectable", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  await tui.waitForText("Other sessions");
  await tui.waitForMatch(/● \/home\/user\/my-project/);

  await tui.selectRow("/home/user/my-project");
  expect(await tui.contextLine()).toContain("session sess-standalone-1");
  // Executing session counted in the status bar.
  expect(await tui.statusBar()).toMatch(/● 1/);
  await tui.screenshot("rust-standalone");
});
