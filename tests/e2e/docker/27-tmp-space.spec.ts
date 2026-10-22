import { test, expect } from "@playwright/test";
import { RustTui, resetMock } from "./helpers/rust-tui";

test.beforeEach(async () => {
  await resetMock();
});

test("a worktree created in the tmp space is grouped apart, and T moves it in and out", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  const suffix = `e2e-tmp-${Date.now() % 1_000_000}`;
  const branch = `feature/${suffix}`;
  const row = new RegExp(`│[▌ ][│├└─ ]*\\S ${branch.replace(/[/.]/g, "\\$&")}\\s`);
  const lineOf = async (re: RegExp) => (await tui.lines()).findIndex((l) => re.test(l));

  // New worktree form: Branch → Name → Base → Tmp space (one repo, so no Repo row).
  await tui.sendKey("n");
  await tui.waitForText("New worktree");
  await tui.waitForText("Tmp space ‹ off ›");
  await tui.type(suffix);
  await tui.waitForGone("checking…", 10_000);
  for (let i = 0; i < 3; i++) await tui.sendKey("Tab");
  await tui.sendKey(" ");
  await tui.waitForText("Tmp space ‹ on ›");
  await tui.waitForText("grouped apart, outside the worktree limit");
  await tui.screenshot("tmp-space-form");
  await tui.sendKey("Enter");
  await tui.waitForGone("New worktree", 10_000);
  await tui.skipStartupScript();

  await tui.waitForMatch(row, 15_000);
  await tui.waitForText("Tmp space ───");
  expect(await lineOf(row)).toBeGreaterThan(await lineOf(/Tmp space ───/));
  expect(await lineOf(/Tmp space ───/)).toBeGreaterThan(await lineOf(/work ───/));
  await tui.screenshot("tmp-space-group");

  // T takes it out (back under work ───, the tmp section goes) and puts it back.
  await tui.selectRow(branch);
  await tui.sendKey("T");
  await tui.waitForText("Moved out of tmp space", 5_000);
  await tui.waitForGone("Tmp space ───", 5_000);
  await tui.waitForMatch(row);
  await tui.sendKey("T");
  await tui.waitForText("Moved to tmp space", 5_000);
  await tui.waitForText("Tmp space ───", 5_000);
  expect(await lineOf(row)).toBeGreaterThan(await lineOf(/Tmp space ───/));

  // The main checkout stays with its repo.
  await tui.selectRow("main");
  await tui.sendKey("T");
  await tui.waitForText("Only linked worktrees can go in the tmp space", 3_000);

  // Clean up.
  await tui.selectRow(branch);
  await tui.sendKey("d");
  await tui.waitForText(`Delete worktree "${branch}"`);
  await tui.sendKey("y");
  await tui.waitForText(`Also delete local branch ${branch}?`);
  await tui.sendKey("y");
  await tui.waitForGone(branch, 20_000);
  await tui.waitForGone("Tmp space ───", 5_000);
});
