import { test, expect } from "@playwright/test";
import { RustTui, resetMock } from "./helpers/rust-tui";

// Settings → Notifications → "Report status to terminal" (`programStatus`):
// opt-in, off by default. The OSC 7501 write itself happens in am-hook on the
// agent's tty and is covered by the Rust tests (tests/hook.rs).

test.beforeEach(async () => {
  await resetMock();
});

// Labels are padded, so the value follows 2+ spaces.
const ROW = /Report status to terminal {2,}(on|off)/;
const HELP = "Tell the terminal (OSC 7501";

async function rowValue(tui: RustTui): Promise<string> {
  const m = ROW.exec(await tui.getScreenText());
  if (!m) throw new Error(`Setting row not found:\n${await tui.getScreenText()}`);
  return m[1];
}

// Move down until the row is selected (its help line shows), so fields added
// above it don't break the spec.
async function selectRow(tui: RustTui): Promise<void> {
  for (let i = 0; i < 60; i++) {
    if ((await tui.getScreenText()).includes(HELP)) return;
    await tui.press("j");
  }
  throw new Error("Report status to terminal row not found");
}

test("Report status to terminal is off by default, toggles on and persists", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();

  await tui.sendKey("s");
  await tui.waitForText("esc close");
  await selectRow(tui);
  expect(await tui.getScreenText()).toContain("Notifications");
  expect(await rowValue(tui)).toBe("off");
  await tui.screenshot("program-status-setting-off");

  await tui.sendKey(" ");
  await tui.waitForMatch(/Report status to terminal {2,}on/);
  expect(await rowValue(tui)).toBe("on");
  await tui.sendKey("Escape");
  await tui.waitForGone("esc close");

  // A new connection is a new am-tui process: the value came from settings.json.
  await page.reload();
  await tui.open();
  await tui.sendKey("s");
  await tui.waitForText("esc close");
  await selectRow(tui);
  expect(await rowValue(tui)).toBe("on");
  await tui.screenshot("program-status-setting-on");

  // Restore.
  await tui.sendKey(" ");
  await tui.waitForMatch(/Report status to terminal {2,}off/);
  expect(await rowValue(tui)).toBe("off");
  await tui.sendKey("Escape");
  await tui.waitForGone("esc close");
});
