import { test, expect } from "@playwright/test";
import { RustTui, resetMock } from "./helpers/rust-tui";

// The entrypoint seeds a fresh `update-check.json` with `latest: 9.9.9`, so the
// background update check reports a newer release without touching the network.

test.beforeEach(async () => {
  await resetMock();
});

const NOTICE = "⬆ v9.9.9 available";
const CHECK = /Check for updates\s+(on|off)/;
const CHECK_HELP = "Show a notice when a newer am release is on GitHub";

// Move down until "Check for updates" is selected (its help line shows), so
// fields added above it don't break the spec.
async function selectCheckRow(tui: RustTui): Promise<void> {
  for (let i = 0; i < 60; i++) {
    if ((await tui.getScreenText()).includes(CHECK_HELP)) return;
    await tui.press("j");
  }
  throw new Error("Check for updates row not found");
}

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
  // Settings opens with a banner naming the release and how to upgrade.
  await tui.waitForText("⬆ am v9.9.9 is available");
  expect(await tui.getScreenText()).toMatch(/Update: .*releases\/latest/);
  await tui.screenshot("update-available-settings");
  expect(await checkValue(tui)).toBe("on");
  await selectCheckRow(tui);
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
  await selectCheckRow(tui);
  await tui.sendKey(" ");
  expect(await checkValue(tui)).toBe("on");
  await tui.sendKey("Escape");
  await tui.waitForGone("esc close");
  await tui.waitForText(NOTICE);
});
