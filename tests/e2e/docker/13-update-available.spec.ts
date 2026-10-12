import { test, expect } from "@playwright/test";
import { RustTui, resetMock } from "./helpers/rust-tui";

// The entrypoint seeds a fresh `update-check.json` with `latest: 9.9.9`, so the
// background update check reports a newer release without touching the network.

test.beforeEach(async () => {
  await resetMock();
});

const NOTICE = "⬆ v9.9.9 available";
const CHECK = /Check for updates\s+(on|off)/;
// "Check for updates" sits 19 rows below the first field ("Open in").
const CHECK_INDEX = 19;

async function checkValue(tui: RustTui): Promise<string> {
  const m = CHECK.exec(await tui.getScreenText());
  if (!m) throw new Error(`Setting row not found:\n${await tui.getScreenText()}`);
  return m[1];
}

test("update notice shows in the Worktrees title and hides when turned off", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();

  // (a) The notice sits right-aligned on the pane's top border.
  await tui.waitForText(NOTICE);
  const top = (await tui.lines()).find((l) => l.includes("─Worktrees")) ?? "";
  expect(top).toContain(NOTICE);
  expect((await tui.lines())[0]).toContain(NOTICE);
  await tui.screenshot("update-available");

  // (b) Settings → Notifications → "Check for updates" off hides it at once.
  await tui.sendKey("s");
  await tui.waitForText("esc close");
  expect(await checkValue(tui)).toBe("on");
  await tui.press("j", CHECK_INDEX);
  await tui.waitForText("Show a notice when a newer am release is on GitHub");
  await tui.sendKey(" ");
  expect(await checkValue(tui)).toBe("off");
  await tui.sendKey("Escape");
  await tui.waitForGone("esc close");
  await tui.waitForGone(NOTICE);
  expect((await tui.lines())[0]).toContain("─Worktrees");
  await tui.screenshot("update-available-off");

  // Restore: turning it back on shows the notice again.
  await tui.sendKey("s");
  await tui.waitForText("esc close");
  await tui.press("j", CHECK_INDEX);
  await tui.waitForText("Show a notice when a newer am release is on GitHub");
  await tui.sendKey(" ");
  expect(await checkValue(tui)).toBe("on");
  await tui.sendKey("Escape");
  await tui.waitForGone("esc close");
  await tui.waitForText(NOTICE);
});
