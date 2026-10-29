import { test, expect } from "@playwright/test";
import { RustTui, FRESH_URL, resetMock, setupMock } from "./helpers/rust-tui";

// Review-only mode (`appMode: "reviews"`): picked on the setup wizard's first
// step, it opens am straight to the review page, with only the page's
// settings, keys and help. The mock answers the review search with
// gh-review-requests.json (#61 on feature/on-origin first, under "Asked you").

const WELCOME = "Welcome to agent-monitor";

test.beforeEach(async () => {
  await resetMock();
});

test("first run in review-only mode: setup, the page alone, pull and remove, back to full", async ({ page }) => {
  await setupMock({ reviews: true });
  const tui = new RustTui(page);
  await tui.openUrl(FRESH_URL);
  await tui.waitForText(WELCOME, 10_000);

  // Mode: review requests only. Its steps skip agent hooks and the context window.
  await tui.waitForText("What do you want am for?");
  await tui.sendKey("j");
  await tui.waitForText("● Review requests only");
  await tui.waitForText("Mode › Editor › Repos › GitHub › Reviews › Privacy");
  await tui.screenshot("review-mode-setup-mode");
  await tui.sendKey("Enter");

  await tui.waitForText("Where should Enter open a pulled PR?");
  await tui.sendKey("Enter");

  // Repositories: at least one is required.
  await tui.waitForText("Add the git repositories whose pull requests you review.");
  await tui.sendKey("Enter");
  await tui.waitForText("Add at least one repository");
  await tui.press("Backspace", 2);
  await tui.type("/work");
  await tui.sendKey("Enter");
  await tui.waitForText("✓ work", 15_000);
  await tui.sendKey("Enter");

  // GitHub: no checkbox, just whether gh is there (the container's fake gh is).
  await tui.waitForText("Review requests come from GitHub");
  await tui.waitForText("✓ gh found");
  await tui.screenshot("review-mode-setup-github");
  await tui.sendKey("Enter");

  // Reviews: no on/off row.
  await tui.waitForText("Which pull requests are waiting on your review?");
  expect(await tui.getScreenText()).not.toContain("Show PRs waiting for my review");
  await tui.sendKey("Enter");

  // Privacy, last in this mode too: ⏎ finishes.
  await tui.waitForText("Help improve am with anonymous usage reports?");
  await tui.waitForText("⏎ finish");
  await tui.sendKey("Enter");

  // The review page, alone.
  await tui.waitForGone(WELCOME);
  await tui.waitForText("─Review requests");
  await tui.waitForText("Add retry to the sync job", 20_000);
  expect(await tui.getScreenText()).not.toContain("─Worktrees");
  expect(await tui.keyBar()).toContain("esc quit");
  await tui.screenshot("review-mode-page");

  // Help: the page's keys only.
  await tui.sendKey("h");
  await tui.waitForText("focus the jobs pane");
  expect(await tui.getScreenText()).not.toContain("new / delete worktree");
  await tui.screenshot("review-mode-help");
  await tui.sendKey("Escape");
  await tui.waitForGone("focus the jobs pane", 5_000);

  // Settings: no dashboard settings.
  await tui.sendKey("s");
  await tui.waitForText("esc close");
  const settings = await tui.getScreenText();
  expect(settings).toContain("Requested from");
  expect(settings).not.toContain("Sort order");
  expect(settings).not.toContain("Harness plugins");
  await tui.screenshot("review-mode-settings");
  await tui.sendKey("Escape");
  await tui.waitForGone("esc close");

  // t pulls #61 into Reviewing, and d removes it again (with its local
  // branch, so other specs still see feature/on-origin as origin-only).
  await tui.waitForText("1 of 3");
  await tui.sendKey("t");
  await tui.waitForText("✓ pulled", 20_000);
  await tui.screenshot("review-mode-pulled");
  await tui.sendKey("d");
  await tui.waitForText('Delete worktree "feature/on-origin"');
  await tui.sendKey("y");
  await tui.waitForText("Also delete local branch feature/on-origin?");
  await tui.sendKey("y");
  await tui.waitForGone("✓ pulled", 20_000);

  // Settings → Mode full: the page stays, and esc now shows the dashboard.
  await tui.sendKey("s");
  await tui.waitForText("reviews: open straight to the review page");
  await tui.sendKey("ArrowLeft");
  await tui.waitForMatch(/Mode\s+full/);
  await tui.waitForText("Fetch PRs"); // the full settings are back
  await tui.sendKey("Escape");
  await tui.waitForGone("esc close");
  expect(await tui.keyBar()).toContain("esc back");
  await tui.sendKey("Escape");
  await tui.waitForText("─Worktrees", 10_000);
  await tui.screenshot("review-mode-back-to-full");
});

test("switching the seeded dashboard to review-only mode and back", async ({ page }) => {
  await setupMock({ reviews: true });
  const tui = new RustTui(page);
  await tui.open();

  // Mode is the first setting.
  await tui.sendKey("s");
  await tui.waitForText("reviews: open straight to the review page");
  await tui.sendKey("ArrowRight");
  await tui.waitForMatch(/Mode\s+reviews/);
  await tui.waitForGone("Sort order");
  await tui.sendKey("Escape");
  await tui.waitForText("─Review requests");
  await tui.waitForText("Add retry to the sync job", 20_000);
  expect(await tui.getScreenText()).not.toContain("─Worktrees");

  // Restore: back to full, then leave the page for the dashboard.
  await tui.sendKey("s");
  await tui.waitForText("reviews: open straight to the review page");
  await tui.sendKey("ArrowLeft");
  await tui.waitForMatch(/Mode\s+full/);
  await tui.sendKey("Escape");
  await tui.waitForGone("esc close");
  await tui.sendKey("Escape");
  await tui.waitForText("─Worktrees", 10_000);

  // Leave nothing cached for the specs that follow.
  await setupMock({ reviews: false });
  await tui.sendKey("v");
  await tui.waitForText("Nothing waiting on your review", 20_000);
  await tui.sendKey("Escape");
  await tui.waitForGone("to review", 5_000);
});
