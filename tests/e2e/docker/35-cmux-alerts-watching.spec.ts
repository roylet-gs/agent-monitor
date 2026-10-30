import { test, expect } from "@playwright/test";
import { RustTui, resetMock } from "./helpers/rust-tui";

// Settings → Notifications → "cmux alerts" (`cmuxNotify`): a checklist of
// what am posts on a worktree's own cmux workspace. There is no cmux in the
// container, so this checks the setting itself (default
// `ci,review,conflicts`) and the help overlay's `⧗ watching` badge legend; the
// badge on a row is covered by the `an_agent_only_watching_ci_says_so_on_its_row_and_session`
// unit test (seeding it would change rows other specs assert on).

test.beforeEach(async () => {
  await resetMock();
});

// The help line under the list names the field under the cursor.
const CMUX_ALERTS_HELP = "changes posted as notifications on the worktree's own workspace";
// Values are cut to the box's width, so match their start.
const ROW = /cmux alerts {2,}(\S[^│\n]*)/;

async function rowValue(tui: RustTui): Promise<string> {
  const m = ROW.exec(await tui.getScreenText());
  if (!m) throw new Error(`cmux alerts row not found:\n${await tui.getScreenText()}`);
  return m[1];
}

async function goToCmuxAlerts(tui: RustTui) {
  for (let i = 0; i < 80; i++) {
    if ((await tui.getScreenText()).includes(CMUX_ALERTS_HELP)) return;
    await tui.sendKey("j");
  }
  throw new Error(`never reached the cmux alerts row:\n${await tui.getScreenText()}`);
}

test("Settings → cmux alerts is a checklist that saves each tick", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();

  await tui.sendKey("s");
  await tui.waitForText("esc close");
  await goToCmuxAlerts(tui);
  expect(await rowValue(tui)).toMatch(/^CI started failing, approved \/ changes/);
  await tui.screenshot("cmux-alerts-row");

  // ⏎ opens the checklist: the last three ticked by default.
  await tui.sendKey("Enter");
  await tui.waitForText("cmux alerts: post the ticked on the worktree's cmux workspace");
  await tui.waitForText("[ ] agent waiting for you");
  const text = await tui.getScreenText();
  expect(text).toContain("[ ] agent finished");
  expect(text).toContain("[x] CI started failing");
  expect(text).toContain("[x] approved / changes requested");
  expect(text).toContain("[x] merge conflicts");
  await tui.screenshot("cmux-alerts-checklist");

  // Space ticks the first one and saves at once.
  await tui.sendKey(" ");
  await tui.waitForText("[x] agent waiting for you");
  await tui.screenshot("cmux-alerts-checklist-ticked");
  await tui.sendKey("Escape");
  await tui.waitForGone("post the ticked");
  await tui.waitForMatch(/cmux alerts {2,}agent waiting for you, CI started failing/);
  await tui.screenshot("cmux-alerts-row-ticked");

  // It persisted: a new am-tui process reads it back.
  await tui.sendKey("Escape");
  await tui.waitForGone("esc close");
  await page.reload();
  await tui.open();
  await tui.sendKey("s");
  await tui.waitForText("esc close");
  await goToCmuxAlerts(tui);
  expect(await rowValue(tui)).toMatch(/^agent waiting for you, CI started failing/);

  // Restore the default (`ci,review,conflicts`).
  await tui.sendKey("Enter");
  await tui.waitForText("[x] agent waiting for you");
  await tui.sendKey(" ");
  await tui.waitForText("[ ] agent waiting for you");
  await tui.sendKey("Escape");
  await tui.waitForGone("post the ticked");
  expect(await rowValue(tui)).toMatch(/^CI started failing, approved \/ changes/);
  await tui.sendKey("Escape");
  await tui.waitForGone("esc close");
});

test("h explains the ⧗ watching badge", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();

  await tui.sendKey("h");
  await tui.waitForText(" Keys ");
  // The line wraps in the help box, so match the part that fits on one row.
  await tui.waitForMatch(/⧗ watching\s+busy, but only waiting on something outside \(gh run watch/);
  expect(await tui.getScreenText()).toContain("Monitor, sleep)");
  await tui.screenshot("help-watching");
  await tui.sendKey("h");
  await tui.waitForGone(" Keys ");
});
