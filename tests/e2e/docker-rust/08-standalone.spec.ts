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
  await tui.waitForMatch(/│Other sessions ─+/);
  await tui.waitForMatch(/● \/home\/user\/my-project/);

  // The session row sits below the "Other sessions" header.
  const lines = await tui.lines();
  const header = lines.findIndex((l) => l.includes("Other sessions"));
  const row = lines.findIndex((l) => l.includes("/home/user/my-project"));
  expect(header).toBeGreaterThanOrEqual(0);
  expect(row).toBeGreaterThan(header);

  await tui.selectRow("/home/user/my-project");
  // Executing (●) standalone session.
  expect(await tui.selectedRow()).toMatch(/│▌● \/home\/user\/my-project/);
  await tui.screenshot("rust-standalone");
});
