import { expect, test } from "@playwright/test";
import { RustTui, resetMock } from "./helpers/rust-tui";

test.beforeEach(async () => {
  await resetMock();
});

const SETTING_HELP = "Anonymous usage and error reports";
const PROMPT = "Help improve am with anonymous usage reports?";

/** Settings → Usage reports, cycled (⏎) until it reads `value`, then closed. */
async function setUsageReports(tui: RustTui, value: "ask" | "on" | "off") {
  await tui.openSettingFromEnd(SETTING_HELP);
  const row = () => tui.lines().then((ls) => ls.find((l) => l.includes("Usage reports")) ?? "");
  for (let i = 0; i < 3 && !new RegExp(`Usage reports\\s+${value}\\b`).test(await row()); i++) {
    await tui.sendKey("Enter");
  }
  expect(await row()).toMatch(new RegExp(`Usage reports\\s+${value}\\b`));
  await tui.sendKey("Escape");
  await tui.waitForGone("esc close");
}

test("a user set up before usage reports is asked once, and the answer sticks", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  // The seeded dir answered already: no prompt.
  expect(await tui.getScreenText()).not.toContain(PROMPT);

  // Back to undecided, as for someone upgrading from an older am.
  await setUsageReports(tui, "ask");
  await page.reload();
  await tui.waitForText(PROMPT, 15_000);
  await tui.waitForText("Never your code, log text, paths");
  await tui.screenshot("usage-reports-prompt");
  // A stray key doesn't answer it.
  await tui.sendKey("j");
  await tui.waitForText(PROMPT);
  await tui.sendKey("y");
  await tui.waitForGone(PROMPT);
  await tui.waitForText("Usage reports on");

  // Answered: a restart doesn't ask again, and Settings shows it.
  await page.reload();
  await tui.open();
  expect(await tui.getScreenText()).not.toContain(PROMPT);
  await tui.openSettingFromEnd(SETTING_HELP);
  expect((await tui.lines()).find((l) => l.includes("Usage reports"))).toMatch(/Usage reports\s+on\b/);
  await tui.screenshot("usage-reports-setting");
  await tui.sendKey("Escape");

  // "n" turns them off; restore the seeded "off" either way.
  await setUsageReports(tui, "ask");
  await page.reload();
  await tui.waitForText(PROMPT, 15_000);
  await tui.sendKey("n");
  await tui.waitForGone(PROMPT);
  await tui.waitForText("No usage reports");
  await tui.openSettingFromEnd(SETTING_HELP);
  expect((await tui.lines()).find((l) => l.includes("Usage reports"))).toMatch(/Usage reports\s+off\b/);
  await tui.sendKey("Escape");
});
