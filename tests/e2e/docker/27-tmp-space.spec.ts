import { test, expect } from "@playwright/test";
import { RustTui, resetMock } from "./helpers/rust-tui";

// `t` pulls an existing branch (someone else's PR) into the tmp space: a
// window with just the branch, no new branches, no questions. The seeded repo
// has an origin-only `feature/on-origin` and a startup script
// (e2e/entrypoint.sh).

test.beforeEach(async () => {
  await resetMock();
});

test("t refuses a branch that exists nowhere", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  const branch = `nobody/e2e-${Date.now() % 1_000_000}`;

  await tui.sendKey("t");
  await tui.waitForText("Pull into Reviewing");
  await tui.waitForText("⏎ pull  esc cancel");
  expect(await tui.getScreenText()).not.toContain("Name");
  await tui.type(branch);
  await tui.waitForText("✗ not found locally or on origin", 10_000);
  await tui.sendKey("Enter");
  await tui.waitForText("No such branch locally or on origin");
  await tui.screenshot("tmp-space-not-found");
  expect(await tui.getScreenText()).not.toContain("─Jobs");

  await tui.sendKey("Escape");
  await tui.waitForGone("Pull into Reviewing", 5_000);
  expect(await tui.getScreenText()).not.toContain("Reviewing ───");
});

test("t pulls a branch on origin into the tmp space without any questions", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  const branch = "feature/on-origin";
  const row = /│[▌ ][│├└─ ]*\S feature\/on-origin\s/;
  const lineOf = async (re: RegExp) => (await tui.lines()).findIndex((l) => re.test(l));

  // The New Worktree form no longer has a tmp space switch.
  await tui.sendKey("n");
  await tui.waitForText("New worktree");
  expect(await tui.getScreenText()).not.toContain("Reviewing ‹");
  await tui.sendKey("Escape");
  await tui.waitForGone("New worktree", 5_000);

  await tui.sendKey("t");
  await tui.waitForText("Pull into Reviewing");
  await tui.type(branch); // no branch prefix: the name is pasted whole
  await tui.waitForText("✓ exists on origin — Enter will pull it", 10_000);
  await tui.screenshot("tmp-space-pull-form");
  await tui.sendKey("Enter");
  await tui.waitForGone("Pull into Reviewing", 10_000);

  // Neither "branch exists" nor the startup script is asked.
  await tui.waitForMatch(row, 20_000);
  await tui.waitForGone("─Jobs", 10_000);
  const screen = await tui.getScreenText();
  expect(screen).not.toContain("⇥ answer");
  expect(screen).not.toContain("Run the repo's startup script?");
  await tui.waitForText("Reviewing ───");
  expect(await lineOf(row)).toBeGreaterThan(await lineOf(/Reviewing ───/));
  expect(await lineOf(/Reviewing ───/)).toBeGreaterThan(await lineOf(/work ───/));
  await tui.screenshot("tmp-space-group");

  // T no longer moves worktrees in or out.
  await tui.selectRow(branch);
  await tui.sendKey("T");
  await tui.waitForText("─Worktrees");
  expect(await tui.getScreenText()).not.toMatch(/Moved (to|out of) tmp space/);

  // Clean up (the local branch too, so other specs still see it as origin-only).
  await tui.sendKey("d");
  await tui.waitForText(`Delete worktree "${branch}"`);
  await tui.sendKey("y");
  await tui.waitForText(`Also delete local branch ${branch}?`);
  await tui.sendKey("y");
  await tui.waitForGone(branch, 20_000);
  await tui.waitForGone("Reviewing ───", 5_000);
});
