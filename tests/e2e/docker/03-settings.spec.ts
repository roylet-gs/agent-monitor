import { test, expect } from "@playwright/test";
import { RustTui, resetMock } from "./helpers/rust-tui";

test.beforeEach(async () => {
  await resetMock();
});

const RESUME = /Resume last session on open\s+(on|off)/;

async function resumeValue(tui: RustTui): Promise<string> {
  const m = RESUME.exec(await tui.getScreenText());
  if (!m) throw new Error(`Setting row not found:\n${await tui.getScreenText()}`);
  return m[1];
}

test("settings editor opens, a toggle persists across restarts", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();

  await tui.sendKey("s");
  await tui.waitForText("Settings ");
  await tui.waitForText("General");
  await tui.waitForText("esc close");
  await tui.screenshot("settings");

  const before = await resumeValue(tui);
  const after = before === "on" ? "off" : "on";
  await tui.sendKey("j"); // cursor → "Resume last session on open"
  await tui.waitForText("Enter also resumes the worktree's most recent Claude session");
  await tui.sendKey(" ");
  expect(await resumeValue(tui)).toBe(after);
  await tui.sendKey("Escape");
  await tui.waitForGone("esc close");

  // A new connection is a new am-tui process: the value came from settings.json.
  await page.reload();
  await tui.open();
  await tui.sendKey("s");
  await tui.waitForText("esc close");
  expect(await resumeValue(tui)).toBe(after);

  // Restore.
  await tui.sendKey("j");
  await tui.sendKey(" ");
  expect(await resumeValue(tui)).toBe(before);
  await tui.sendKey("Escape");
});
