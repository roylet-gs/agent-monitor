import { test, expect } from "@playwright/test";
import { NPM_URL, RustTui, resetMock } from "./helpers/rust-tui";

// The entrypoint seeds a fresh `update-check.json` with `latest: 9.9.9`, so the
// background update check reports a newer release without touching the network.
// The container's binaries aren't a Homebrew/npm/pnpm install, so on 7681 the
// notice can only say how to update; 7684 runs am-tui as an npm install
// (`AM_INSTALL_METHOD=npm`) with a fake `npm` that only prints.

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
  const top = (await tui.lines()).find((l) => l.includes("· Worktrees")) ?? "";
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
  expect((await tui.lines())[0]).toContain("· Worktrees");
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

test("clicking the notice says how to update an install am can't update itself", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  await tui.waitForText(NOTICE);
  await tui.clickText(NOTICE);
  await tui.waitForText("am v9.9.9 is available");
  // The popup wraps the URL onto its own line.
  const popup = await tui.getScreenText();
  expect(popup).toContain("To update: see");
  expect(popup).toContain("releases/latest");
  await tui.screenshot("update-click-instructions");
  await tui.sendKey("x");
  await tui.waitForGone("To update:");
  expect(await tui.getScreenText()).toContain(NOTICE);
});

test("clicking the notice runs the upgrade in the jobs pane, then restarts am", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.openUrl(NPM_URL);
  await tui.waitForText("· Worktrees", 10_000);
  await tui.waitForText(NOTICE);

  // Asks first, naming the command.
  await tui.clickText(NOTICE);
  await tui.waitForText("Update am v");
  expect(await tui.getScreenText()).toContain("Runs: npm install -g @roylet-gs/agent-monitor@latest");
  await tui.screenshot("update-confirm");

  // ⏎ runs it as a job, its output in the ─Jobs pane; then offers a restart.
  await tui.sendKey("Enter");
  await tui.waitForText("Update am to v9.9.9");
  // The fake npm's output (the pane wraps long lines).
  await tui.waitForText("fake npm install -g");
  await tui.waitForText("added 1 package");
  await tui.waitForText("am v9.9.9 is installed. Restart now");
  await tui.screenshot("update-restart-prompt");
  expect(await tui.getScreenText()).not.toContain(NOTICE);

  // `n` leaves it for later.
  await tui.sendKey("n");
  await tui.waitForGone("Restart now");
  await tui.waitForText("Restart am to use v9.9.9");

  // A restarted am-tui (still the old binary here) finds the release again;
  // this time restart from the prompt and the dashboard comes back.
  await page.reload();
  await tui.waitForText("· Worktrees", 10_000);
  await tui.waitForText(NOTICE);
  await tui.clickText(NOTICE);
  await tui.waitForText("Update am v");
  await tui.sendKey("y");
  await tui.waitForText("am v9.9.9 is installed. Restart now");
  await tui.sendKey("y");
  await tui.waitForGone("Restart now");
  await tui.waitForText("· Worktrees", 10_000);
  await tui.waitForText(NOTICE);
  // A fresh process: the update job's pane went with the old one.
  expect(await tui.getScreenText()).not.toContain("Update am to v9.9.9");
  await tui.screenshot("update-restarted");
});
