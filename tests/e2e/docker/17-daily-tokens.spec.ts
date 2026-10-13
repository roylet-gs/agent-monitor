import { test, expect } from "@playwright/test";
import { RustTui, resetMock } from "./helpers/rust-tui";

// e2e/entrypoint.sh writes two transcripts stamped today:
// - main's session (/work): 150,120 tokens, on two lines sharing one message id
//   (counted once);
// - an untracked /home/user/elsewhere session: 849,880 tokens today, plus a line
//   from two days ago that never counts.
// So "all" shows 1.0M and "visible" shows main's 150k.

test.beforeEach(async () => {
  await resetMock();
});

/** Open settings, move to the field whose help text is `help`, press Enter. */
async function activateSetting(tui: RustTui, help: string): Promise<void> {
  await tui.sendKey("s");
  await tui.waitForText("esc close");
  for (let i = 0; i < 40 && !(await tui.getScreenText()).includes(help); i++) {
    await tui.sendKey("j");
  }
  await tui.waitForText(help);
  await tui.sendKey("Enter");
  await tui.sendKey("Escape");
  await tui.waitForGone("esc close");
}

const SCOPE_HELP = "all: every session today; visible: only the rows shown";
const SHOW_HELP = "Tokens used today (input, cache and output)";

test("today's token usage sits bottom right and follows its scope", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();

  // Default scope "all": every transcript touched today, de-duplicated.
  await tui.waitForMatch(/\d+ of \d+ ─ today 1\.0M ╯/);
  await tui.screenshot("daily-tokens-all");

  // "visible": only usage attributed to a row on screen (main's session).
  await activateSetting(tui, SCOPE_HELP);
  await tui.waitForMatch(/─ today \(visible\) 150k ╯/);
  await tui.screenshot("daily-tokens-visible");

  // Back to "all".
  await activateSetting(tui, SCOPE_HELP);
  await tui.waitForMatch(/─ today 1\.0M ╯/);
});

test("the token total can be turned off", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  await tui.waitForText("today 1.0M");

  await activateSetting(tui, SHOW_HELP);
  await tui.waitForGone("today 1.0M");
  expect(await tui.getScreenText()).not.toContain(" today ");

  // Restore.
  await activateSetting(tui, SHOW_HELP);
  await tui.waitForText("today 1.0M");
});
