import { test, expect } from "@playwright/test";
import { RustTui, FRESH_URL, resetMock } from "./helpers/rust-tui";

// Settings → "Run setup again…" and "Reset settings…". The reset runs on
// FRESH_URL (port 7682, wiped on every connection) so the seeded settings
// other specs rely on are never reset.

const WELCOME = "Welcome to agent-monitor";
const RERUN_HELP = "Walk the setup wizard again";
const RESET_HELP = "Defaults for every setting, then setup";

test.beforeEach(async () => {
  await resetMock();
});

test("run setup again starts from the current settings and esc cancels", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  await tui.waitForText("ENG-123", 15_000);

  await tui.openSetting(RERUN_HELP, 80);
  await tui.waitForText("Run setup again…");
  await tui.sendKey("Enter");
  await tui.waitForText(WELCOME);
  await tui.waitForText("What do you want am for?");
  await tui.waitForText("● Agents & worktrees"); // the saved mode
  await tui.waitForText("esc cancel");
  await tui.screenshot("rerun-setup-mode");
  await tui.sendKey("Escape");
  await tui.waitForGone(WELCOME);
  await tui.waitForText("Setup closed — settings unchanged");

  // Walk it to the end: the repo is listed as added, and the Linear key
  // survives (the ticket column stays).
  await tui.openSetting(RERUN_HELP, 80);
  await tui.sendKey("Enter");
  await tui.waitForText("What do you want am for?");
  await tui.sendKey("Enter");
  await tui.waitForText("Where should Enter open a worktree?");
  await tui.sendKey("Enter");
  await tui.waitForText("am learns what each agent is doing");
  await tui.sendKey((await tui.getScreenText()).includes("Hooks are not installed yet") ? "n" : "Enter");
  await tui.waitForText("Add the git repositories whose worktrees you want to watch.");
  await tui.waitForText("✓ work");
  await tui.screenshot("rerun-setup-repos");
  await tui.sendKey("Enter");
  await tui.waitForText("GitHub PR & CI status (via gh)");
  await tui.sendKey("Enter");
  await tui.waitForText("Which pull requests are waiting on your review?");
  // Me and my teams: setup makes a group for each kind of request.
  await tui.waitForText("Groups: Asked you, Asked your team");
  await tui.sendKey("Enter");
  await tui.waitForText("How many tokens of context should the bar measure against?");
  await tui.sendKey("Enter");
  await tui.waitForGone(WELCOME);
  await tui.waitForText("Setup complete");
  await tui.waitForText("ENG-123", 15_000);

  // The groups are on the review page's groups list; remove them again so
  // the seeded settings stay as other specs expect.
  await tui.sendKey("v");
  await tui.waitForText("─Review requests");
  await tui.sendKey("f");
  await tui.waitForText("Review filters");
  await tui.sendKey("ArrowDown");
  await tui.sendKey("Enter");
  await tui.waitForText("Asked your team");
  for (let i = 0; i < 2; i++) {
    await tui.sendKey("x");
    await tui.sendKey("y");
  }
  await tui.waitForGone("Asked your team");
  await tui.press("Escape", 3); // groups page, filters, review page
  await tui.waitForText("─Worktrees");
});

test("reset settings asks first, then reopens setup", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.openUrl(FRESH_URL);
  await tui.waitForText(WELCOME, 10_000);
  // A first run can't be cancelled.
  expect(await tui.getScreenText()).not.toContain("esc cancel");
  await tui.sendKey("Enter"); // mode: full
  await tui.waitForText("Where should Enter open a worktree?");
  await tui.sendKey("Enter"); // editor
  await tui.waitForText("am learns what each agent is doing");
  await tui.sendKey("n"); // skip hooks
  await tui.waitForText("Add the git repositories whose worktrees you want to watch.");
  await tui.sendKey("Enter"); // no repo
  await tui.waitForText("GitHub PR & CI status (via gh)");
  await tui.sendKey("Enter");
  await tui.waitForText("Which pull requests are waiting on your review?");
  await tui.sendKey("Enter");
  await tui.waitForText("How many tokens of context should the bar measure against?");
  await tui.sendKey("Enter");
  await tui.waitForGone(WELCOME);

  await tui.openSetting(RESET_HELP, 80);
  await tui.sendKey("Enter");
  await tui.waitForText("Put every setting back to its default");
  await tui.screenshot("reset-settings-confirm");
  await tui.sendKey("n");
  await tui.waitForGone("Put every setting back to its default");
  await tui.waitForText("esc close"); // still in settings

  await tui.sendKey("Enter");
  await tui.waitForText("Put every setting back to its default");
  await tui.sendKey("y");
  await tui.waitForText(WELCOME);
  await tui.waitForText("What do you want am for?");
  // setupCompleted went with the reset, so this is a first run again.
  expect(await tui.getScreenText()).not.toContain("esc cancel");
  await tui.screenshot("reset-settings-setup");
});
