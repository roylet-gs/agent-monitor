import { test, expect } from "@playwright/test";
import { RustTui, resetMock, setupMock } from "./helpers/rust-tui";

// PRs waiting on your review: the `N to review` badge on the ─Worktrees
// border, the `v` page (`V` beside the list, `ctrl-v` in a new window), and
// `t` pulling one into Reviewing (`T` also running the repo's review script,
// added from its template on the way). The mock answers
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
  await tui.screenshot("dashboard-review-badge");

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
  expect(await tui.keyBar()).toContain("⏎ pull");
  // The bottom border sums up who's asked and the groups' filters.
  expect(screen).toMatch(/╰ you \+ your teams ─/);
  await tui.screenshot("review-page");
  // The same badge sits in the page's own top-right border corner.
  expect((await tui.lines())[0]).toMatch(/─Review requests.* 3 to review ╮$/);
  await tui.screenshot("review-page-badge");

  // f opens just the review filters over the page; esc goes back to it.
  await tui.sendKey("f");
  await tui.waitForText("Review filters");
  const filters = await tui.getScreenText();
  expect(filters).toContain("Requested from");
  expect(filters).toContain("Groups…");
  expect(filters).not.toContain("Hide drafts");
  expect(filters).not.toContain("Theme");
  await tui.screenshot("review-filters");
  await tui.sendKey("Escape");
  await tui.waitForGone("Review filters");
  expect(await tui.getScreenText()).toContain("─Review requests");

  // g opens the PR (no browser in the container: the opener's error pops up).
  expect(await tui.keyBar()).toContain("⏎ pull  g PR");
  await tui.sendKey("g");
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

  // ⏎ on #61 pulls its branch into Reviewing (like t), with no questions,
  // and the page stays open (so more can be pulled) while the row shows progress.
  await tui.sendKey("k");
  await tui.waitForText("1 of 3");
  await tui.sendKey("Enter");
  await tui.waitForMatch(/pulling…|✓ pulled/, 10_000);
  // The pull shows on its row; no jobs pane opens beside the page.
  expect(await tui.getScreenText()).not.toContain("─Jobs");
  await tui.waitForText("✓ pulled", 20_000);
  expect(await tui.getScreenText()).toContain("─Review requests");
  expect(await tui.getScreenText()).not.toContain("─Jobs");
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

test("T pulls a PR and runs the repo's review script, offering to add one first", async ({ page }) => {
  await setupMock({ reviews: true, linearByBranch: { "feature/no-ticket": null, "patch-1": null } });
  const tui = new RustTui(page);
  await tui.open();
  const branch = "feature/on-origin";
  await tui.sendKey("v");
  await tui.waitForText("Add retry to the sync job", 20_000);
  await tui.waitForText("1 of 3");
  // T is on the bar, or behind Shift where the terminal reports held keys.
  expect(await tui.keyBar()).toMatch(/T pull \+ script|⇧\/ctrl more/);

  // /work has no review script: T asks to add one, and n cancels.
  await tui.sendKey("T");
  await tui.waitForText("has no review script yet. Add one?");
  await tui.screenshot("review-add-script");
  await tui.sendKey("n");
  await tui.waitForGone("has no review script yet");
  expect(await tui.getScreenText()).not.toContain("pulling…");

  // y makes it from the template and opens it in the editor (no VS Code in
  // the container: the opener's error pops up) while the PR is pulled.
  await tui.sendKey("T");
  await tui.waitForText("has no review script yet. Add one?");
  await tui.sendKey("y");
  await tui.waitForText("Couldn't open", 10_000);
  await tui.sendKey("x"); // any key closes the popup
  await tui.waitForGone("Couldn't open");
  // Pulled, the job waits for the script to be saved…
  await tui.waitForText("save the review script to run it", 20_000);
  await tui.screenshot("review-script-waiting");

  // …or runs now from the dashboard's jobs pane. It would open in a new
  // terminal window; the container has none, so a popup says it runs in the
  // jobs pane instead, where its output (with the PR from its environment) shows.
  await tui.sendKey("Escape");
  await tui.waitForText("Runs the review script once you save it", 10_000);
  await tui.sendKey("Tab");
  await tui.sendKey("Enter");
  await tui.waitForText("Couldn't open a terminal window", 10_000);
  await tui.screenshot("review-script-no-window");
  await tui.sendKey("x"); // any key closes the popup
  await tui.waitForGone("Couldn't open a terminal window");
  await tui.waitForText("Review script finished for #61.", 20_000);
  await tui.screenshot("review-script-output");

  // Already pulled: T refuses instead of running the script again.
  await tui.sendKey("v");
  await tui.waitForText("✓ pulled");
  await tui.sendKey("T");
  await tui.waitForText(`${branch} is already pulled`);

  // Restore: remove the worktree with its local branch, the review script,
  // and the cached list.
  await tui.sendKey("d");
  await tui.waitForText(`Delete worktree "${branch}"`);
  await tui.sendKey("y");
  await tui.waitForText(`Also delete local branch ${branch}?`);
  await tui.sendKey("y");
  await tui.waitForGone("✓ pulled", 20_000);
  await tui.sendKey("Escape");
  await tui.openSetting("Add or stop tracking repositories, edit their startup and review scripts");
  await tui.sendKey("Enter");
  await tui.waitForText("Repositories — a add · x remove");
  await tui.sendKey("s");
  await tui.waitForMatch(/Review script\s+✓ added/);
  await tui.sendKey("ArrowDown");
  await tui.sendKey("x");
  await tui.waitForText("Removed the review script for work");
  await tui.press("Escape", 3); // the chooser, the repositories page, Settings
  await tui.waitForGone("esc close");
  await setupMock({ reviews: false });
  await tui.sendKey("v");
  await tui.sendKey("r");
  await tui.waitForText("Nothing waiting on your review", 20_000);
});

test("review groups: a group takes its PRs first, with autocomplete and a badge", async ({ page }) => {
  await setupMock({ reviews: true, linearByBranch: { "feature/no-ticket": null, "patch-1": null } });
  const tui = new RustTui(page);
  await tui.open();
  await tui.sendKey("v");
  await tui.waitForText("Update onboarding copy", 20_000);

  // f → Groups… (second row) → a adds a group and opens its form.
  await tui.sendKey("f");
  await tui.waitForText("Review filters");
  await tui.sendKey("ArrowDown");
  await tui.sendKey("Enter");
  await tui.waitForText("Review groups — a add");
  await tui.sendKey("a");
  await tui.waitForText("Older than (days)");
  await tui.sendKey("Enter"); // Name
  await tui.type("Carol");
  await tui.sendKey("Enter");
  // Authors (under Order): suggestions come from the repo's assignable
  // users (gh-assignable-users.json) plus the PRs' authors.
  await tui.press("ArrowDown", 3); // Name → On → Order → Authors
  await tui.sendKey("Enter");
  await tui.waitForText("⇥ complete");
  await tui.type("ca");
  await tui.waitForText("carol (Carol Danvers) · cassie", 10_000);
  await tui.screenshot("review-group-suggest");
  await tui.sendKey("Tab");
  await tui.waitForText("carol, ▏");
  await tui.sendKey("Enter");
  await tui.waitForText("@carol");
  await tui.sendKey("Escape"); // the form
  await tui.waitForText("by @carol");
  await tui.screenshot("review-groups");
  await tui.sendKey("Escape"); // the groups page
  await tui.sendKey("Escape"); // the filters
  await tui.waitForGone("Review filters");

  // #62 (carol, only asks your team) now opens the page in its own group.
  await tui.waitForText("Carol ─");
  const lineOf = async (re: RegExp) => (await tui.lines()).findIndex((l) => re.test(l));
  expect(await lineOf(/Carol ─/)).toBeLessThan(await lineOf(/Update onboarding copy/));
  expect(await lineOf(/Update onboarding copy/)).toBeLessThan(await lineOf(/Asked you ─/));
  expect(await tui.getScreenText()).toContain("· 1 group");
  await tui.screenshot("review-page-groups");

  // The dashboard badge counts each group shown on it.
  await tui.sendKey("Escape");
  await tui.waitForText("Carol 1 · 2 to review");

  // Restore: remove the group.
  await tui.sendKey("v");
  await tui.sendKey("f");
  await tui.waitForText("Review filters");
  await tui.sendKey("ArrowDown");
  await tui.sendKey("Enter");
  await tui.waitForText("by @carol");
  await tui.sendKey("x");
  await tui.sendKey("y");
  await tui.waitForGone("by @carol");
  await tui.press("Escape", 2); // the groups page, the filters
  await tui.waitForGone("Carol ─");
  await setupMock({ reviews: false });
  await tui.sendKey("r");
  await tui.waitForText("Nothing waiting on your review", 20_000);
});

test("V puts the review page beside the list; ctrl-v opens it in a new window", async ({ page }) => {
  await setupMock({ reviews: true, linearByBranch: { "feature/no-ticket": null, "patch-1": null } });
  const tui = new RustTui(page);
  await tui.open();
  await tui.waitForText("3 to review", 20_000);
  // t no longer pulls from the dashboard.
  expect(await tui.keyBar()).not.toContain("pull");

  await tui.sendKey("V");
  await tui.waitForText("─Review requests");
  await tui.waitForText("Add retry to the sync job");
  // Both panes side by side on the same rows.
  const top = (await tui.lines())[0];
  expect(top).toMatch(/─Worktrees .*─Review requests/);
  expect(await tui.keyBar()).toContain("⇥ list");
  expect(await tui.keyBar()).toContain("l ticket");
  await tui.screenshot("review-split");

  // ⇥ hands the keys to the list: j moves the worktree selection.
  await tui.sendKey("Tab");
  await tui.waitForMatch(/⇥ reviews/);
  const before = await tui.selectedName();
  await tui.sendKey("j");
  expect(await tui.selectedName()).not.toBe(before);
  expect(await tui.getScreenText()).toContain("─Review requests");

  // Back to the review page; esc closes the split.
  await tui.sendKey("Tab");
  await tui.waitForMatch(/⇥ list/);
  await tui.sendKey("Escape");
  await tui.waitForGone("─Review requests", 5_000);
  expect(await tui.getScreenText()).toContain("─Worktrees");

  // ctrl-v: no terminal emulator in the container, so the launch error pops up.
  await tui.sendKey("Control+v");
  await tui.waitForText("Couldn't open a terminal window", 10_000);
  await tui.screenshot("review-window-error");
  await tui.sendKey("Escape");
  await tui.waitForGone("Couldn't open a terminal window", 5_000);

  // Empty the cached list for the specs that follow.
  await setupMock({ reviews: false });
  await tui.sendKey("v");
  await tui.sendKey("r");
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
