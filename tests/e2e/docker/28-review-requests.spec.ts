import { test, expect } from "@playwright/test";
import { RustTui, resetMock, setupMock } from "./helpers/rust-tui";

// PRs waiting on your review: the `N to review` badge on the ─Worktrees
// border, the `v` page, and `t` pulling one into Reviewing. The mock answers
// the review search with gh-review-requests.json: three acme/work PRs. In the
// default order (`reviewSort: direct`) the ones asking you by name come first,
// newest first — #61 (head feature/on-origin, ticket ENG-123), #63 (from a
// fork, no ticket) — then "Asked your team": #62 (no ticket).

test.beforeEach(async () => {
  await resetMock();
});

test("review requests: badge, page, open, ticket, pull into Reviewing", async ({ page }) => {
  await setupMock({ reviews: true, linearByBranch: { "feature/no-ticket": null, "patch-1": null } });
  const tui = new RustTui(page);
  await tui.open();
  const branch = "feature/on-origin";
  const row = /│[▌ ][│├└─ ]*\S feature\/on-origin\s/;

  // The badge sits in the top-right corner of the border (after the update notice).
  await tui.waitForText("3 to review", 20_000);
  expect((await tui.lines())[0]).toMatch(/─Worktrees ─.* 3 to review ╮$/);
  expect(await tui.keyBar()).toContain("v reviews");
  await tui.screenshot("review-badge");

  await tui.sendKey("v");
  await tui.waitForText("─Review requests");
  await tui.waitForText("Add retry to the sync job");
  const screen = await tui.getScreenText();
  expect(screen).toContain("Update onboarding copy");
  expect(screen).toContain("@platform");
  expect(screen).toMatch(/Fix typo in README +@outsider fork/);
  expect(screen).toContain("ENG-123");
  expect(screen).toContain("1 of 3");
  const lineOf = async (re: RegExp) => (await tui.lines()).findIndex((l) => re.test(l));
  expect(await lineOf(/Asked you ─/)).toBeLessThan(await lineOf(/Add retry to the sync job/));
  expect(await lineOf(/Fix typo in README/)).toBeLessThan(await lineOf(/Asked your team ─/));
  expect(await lineOf(/Asked your team ─/)).toBeLessThan(await lineOf(/Update onboarding copy/));
  expect(await tui.keyBar()).toContain("t pull into Reviewing");
  // The bottom border sums up the search's filters.
  expect(screen).toContain("you + your teams · no drafts · not yet reviewed");
  await tui.screenshot("review-page");

  // f opens just the review filters over the page; esc goes back to it.
  await tui.sendKey("f");
  await tui.waitForText("Review filters");
  const filters = await tui.getScreenText();
  expect(filters).toContain("Requested from");
  expect(filters).toContain("Extra search filters");
  expect(filters).not.toContain("Theme");
  await tui.screenshot("review-filters");
  await tui.sendKey("Escape");
  await tui.waitForGone("Review filters");
  expect(await tui.getScreenText()).toContain("─Review requests");

  // ⏎ opens the PR (no browser in the container: the opener's error pops up).
  await tui.sendKey("Enter");
  await tui.waitForMatch(/✗ |Opened https:\/\/github\.com\/acme\/work\/pull\/61/, 10_000);
  if ((await tui.getScreenText()).includes("✗ ")) {
    await tui.sendKey("x"); // any key closes the popup
  }

  // #63 has no ticket, and it's from a fork so it can't be pulled.
  await tui.sendKey("j");
  await tui.waitForText("2 of 3");
  await tui.sendKey("l");
  await tui.waitForText("No Linear ticket for this PR");
  await tui.sendKey("t");
  await tui.waitForText("#63 is from a fork");
  expect(await tui.getScreenText()).toContain("─Review requests");

  // t on #61 pulls its branch into Reviewing, with no questions, and the
  // page stays open (so more can be pulled) while the row shows progress.
  await tui.sendKey("k");
  await tui.waitForText("1 of 3");
  await tui.sendKey("t");
  await tui.waitForMatch(/pulling…|✓ pulled/, 10_000);
  await tui.waitForText("✓ pulled", 20_000);
  expect(await tui.getScreenText()).toContain("─Review requests");
  expect(await tui.getScreenText()).not.toContain("Run the repo's startup script?");
  await tui.screenshot("review-pulled");

  // ⏎ on a pulled PR opens its worktree in the IDE, like on the dashboard
  // (no VS Code in the container: the opener's error pops up).
  expect(await tui.keyBar()).toContain("⏎ open  g PR");
  await tui.sendKey("Enter");
  await tui.waitForText("Couldn't open", 10_000);
  await tui.sendKey("x"); // any key closes the popup
  await tui.waitForGone("Couldn't open");

  // t again selects its worktree, under Reviewing.
  await tui.sendKey("t");
  await tui.waitForText(`${branch} is already checked out`);
  await tui.waitForMatch(row);
  await tui.waitForText("Reviewing ───");
  expect(await tui.selectedName()).toContain(branch);

  // d on the page removes it again (the local branch too, so other specs
  // still see it as origin-only), without leaving the page.
  await tui.sendKey("v");
  await tui.waitForText("✓ pulled");
  await tui.sendKey("d");
  await tui.waitForText(`Delete worktree "${branch}"`);
  await tui.sendKey("y");
  await tui.waitForText(`Also delete local branch ${branch}?`);
  await tui.sendKey("y");
  await tui.waitForGone("✓ pulled", 20_000);
  expect(await tui.getScreenText()).toContain("─Review requests");
  await tui.sendKey("Escape");
  await tui.waitForGone("Reviewing ───", 20_000);

  // Nothing waiting any more: the badge goes, and the cached list is emptied
  // for the specs that follow.
  await setupMock({ reviews: false });
  await tui.sendKey("v");
  await tui.waitForText("Nothing waiting on your review", 20_000);
  await tui.sendKey("Escape");
  await tui.waitForGone("to review", 5_000);
});

test("a GitHub rate limit is explained, not left loading", async ({ page }) => {
  await setupMock({ rateLimited: true });
  const tui = new RustTui(page);
  await tui.open();
  await tui.sendKey("v");
  await tui.waitForText("─Review requests");
  await tui.waitForText("GitHub has rate-limited you — resets in", 20_000);
  const screen = await tui.getScreenText();
  expect(screen).not.toContain("Loading…");
  expect(screen).toContain("API rate limit exceeded");
  await tui.waitForText("⚠ GitHub rate-limited — cached", 20_000);
  await tui.screenshot("review-rate-limited");

  // Once GitHub answers again, r brings the page back.
  await setupMock({ rateLimited: false });
  await tui.sendKey("r");
  await tui.waitForText("Nothing waiting on your review", 20_000);
  await tui.waitForGone("rate-limited", 20_000);
});
