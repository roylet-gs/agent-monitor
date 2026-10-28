import { test, expect } from "@playwright/test";
import { RustTui, resetMock, setupMock } from "./helpers/rust-tui";

test.beforeEach(async () => {
  await resetMock();
});

/** The `─Jobs` pane's top border is on screen, with the job still asking. */
async function expectJobsPane(tui: RustTui): Promise<void> {
  const text = await tui.getScreenText();
  expect(text).toContain("─Jobs");
  expect(text).toContain("Run the repo's startup script?");
}

test("the jobs pane stays in view on the review page, Settings and help", async ({ page }) => {
  await setupMock({ reviews: true });
  const tui = new RustTui(page);
  await tui.open();

  // A create waits on the startup-script question, keeping the pane open.
  const suffix = `e2e-jobs-${Date.now() % 1_000_000}`;
  const branch = `feature/${suffix}`;
  await tui.sendKey("n");
  await tui.waitForText("New worktree");
  await tui.type(suffix);
  await tui.waitForGone("checking…", 10_000);
  await tui.sendKey("Enter");
  await tui.waitForText("Run the repo's startup script?", 30_000);
  await tui.waitForText("─Jobs");

  // The full review page takes the list's place; the pane stays beside it.
  await tui.sendKey("v");
  await tui.waitForText("─Review requests");
  await tui.waitForText("#61");
  await expectJobsPane(tui);
  const top = (await tui.lines())[0];
  expect(top).toContain("─Review requests");
  expect(top).toContain("─Jobs");
  expect(await tui.keyBar()).toContain("⇥ answer");
  await tui.screenshot("jobs-pane-review-page");

  // ⇥ focuses the pane, ⇥ again goes back to the page.
  await tui.sendKey("Tab");
  await tui.waitForMatch(/back to reviews/);
  await tui.screenshot("jobs-pane-review-page-focused");
  await tui.sendKey("Tab");
  await tui.waitForGone("back to reviews");
  expect(await tui.keyBar()).toContain("⇥ answer");
  await tui.waitForText("─Review requests");

  await tui.sendKey("Escape");
  await tui.waitForGone("─Review requests");
  await tui.waitForText("─Worktrees");

  // Settings is drawn beside the pane, not over it.
  await tui.sendKey("s");
  await tui.waitForText("Settings ");
  await tui.waitForText("esc close");
  await expectJobsPane(tui);
  await tui.screenshot("jobs-pane-settings");
  await tui.sendKey("Escape");
  await tui.waitForGone("esc close");

  // So is help.
  await tui.sendKey("h");
  await tui.waitForText(" Keys ");
  await expectJobsPane(tui);
  await tui.screenshot("jobs-pane-help");
  await tui.sendKey("Escape");
  await tui.waitForGone(" Keys ");

  // Restore: decline the script, then delete the worktree and its branch.
  await tui.skipStartupScript();
  await tui.waitForMatch(new RegExp(`│[▌ ][│├└─ ]*\\S ${branch.replace(/[/.]/g, "\\$&")}\\s`), 15_000);
  await tui.selectRow(branch);
  await tui.sendKey("d");
  await tui.waitForText(`Delete worktree "${branch}"`);
  await tui.sendKey("y");
  await tui.waitForText(`Also delete local branch ${branch}?`);
  await tui.sendKey("y");
  await tui.waitForGone(branch, 20_000);
});
