import { test, expect } from "@playwright/test";
import { RustTui, resetMock } from "./helpers/rust-tui";

// The container's repo has a local bare `origin` with a branch that exists
// only there (feature/on-origin), and a startup script (see e2e/entrypoint.sh).
// Creates run as jobs in the right-hand `─Jobs` pane; its questions are
// answered after Tab focuses it.

test.beforeEach(async () => {
  await resetMock();
});

test("a branch only on origin is pulled without asking; with a local copy too the jobs pane asks", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  const branch = "feature/on-origin";
  const row = /│[▌ ][│├└─ ]*\S feature\/on-origin\s/;
  const deleteRow = async (alsoBranch: boolean) => {
    await tui.selectRow(branch);
    await tui.sendKey("d");
    await tui.waitForText(`Delete worktree "${branch}"`);
    await tui.sendKey("y");
    await tui.waitForText(`Also delete local branch ${branch}?`);
    await tui.sendKey(alsoBranch ? "y" : "n");
    await tui.waitForGone(branch, 20_000);
  };

  await tui.sendKey("n");
  await tui.waitForText("New worktree");
  await tui.waitForText("tab next  ⏎ create  esc cancel");
  // Branch is prefilled with the default prefix.
  await tui.waitForMatch(/Branch\s+feature\/▏/);

  // Only on origin (someone else's PR): Enter pulls it, no branch question.
  await tui.type("on-origin");
  await tui.waitForText("✓ exists on origin — Enter will pull it", 10_000);
  await tui.waitForText("Base      tracks origin/feature/on-origin");
  await tui.screenshot("new-worktree-on-origin");
  await tui.sendKey("Enter");
  await tui.waitForGone("New worktree", 5_000);
  await tui.skipStartupScript(); // a normal create still offers the script
  expect(await tui.getScreenText()).not.toContain('Branch "feature/on-origin" exists');
  await tui.waitForMatch(row, 15_000);
  await tui.screenshot("on-origin-pulled");

  // Keep the local branch: now pulling would reset it, so the pane asks.
  await deleteRow(false);
  await tui.sendKey("n");
  await tui.waitForText("New worktree");
  await tui.type("on-origin");
  await tui.waitForText("✓ on origin and locally — Enter will ask first", 10_000);
  await tui.sendKey("Enter");
  // No blocking box: the question shows in the jobs pane, with a
  // placeholder row in the list, and the list keeps its keys.
  await tui.waitForGone("New worktree", 5_000);
  await tui.waitForText("─Jobs", 10_000);
  await tui.waitForText('Branch "feature/on-origin" exists', 15_000);
  await tui.waitForText("Pull remote (reset local to origin)");
  await tui.waitForText("feature/on-origin waiting — ⇥ to answer");
  expect(await tui.keyBar()).toContain("⇥ answer");
  await tui.screenshot("branch-exists-jobs-pane");

  await tui.sendKey("Tab");
  await tui.waitForText("⇥ back  x dismiss");
  await tui.sendKey("p"); // pull
  await tui.skipStartupScript();
  await tui.waitForMatch(row, 15_000);

  // Clean up, local branch too, so the branch is origin-only again.
  await deleteRow(true);
});

test("creating a fresh worktree adds and selects its row", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  const name = `e2e-new-${Date.now() % 1_000_000}`;

  await tui.sendKey("n");
  await tui.waitForText("New worktree");
  await tui.type(name);
  // The advisory check settles with no indicator for a brand-new branch.
  await tui.waitForGone("checking…", 10_000);
  const form = await tui.getScreenText();
  expect(form).not.toContain("exists on origin");
  expect(form).toMatch(/Base\s+main/);

  await tui.sendKey("Enter");
  await tui.waitForGone("New worktree", 10_000);
  await tui.waitForText("─Jobs", 5_000);
  await tui.waitForText(`Create feature/${name}`);
  // Created: the pane asks about the repo's startup script.
  await tui.waitForText("Run the repo's startup script?", 30_000);
  await tui.waitForText(`feature/${name}`, 15_000);
  // pending_select puts the new row under the cursor.
  await tui.waitForMatch(new RegExp(`│▌[│├└─ ]*\\S feature/${name}\\s`), 10_000);
  expect(await tui.selectedName()).toBe(`feature/${name}`);
  await tui.screenshot("worktree-created-script-question");

  // No terminal emulator in the container: the script runs here, its
  // output (ANSI stripped) streamed into the pane.
  await tui.sendKey("Tab");
  await tui.sendKey("y");
  await tui.waitForText("e2e startup script running in", 15_000);
  await tui.waitForText("e2e startup script done", 15_000);
  const screen = await tui.getScreenText();
  expect(screen).toContain("coloured line");
  expect(screen).not.toContain("[32m");
  await tui.screenshot("startup-script-output");
  // A finished job with output lingers, then the pane closes by itself.
  await tui.waitForGone("─Jobs", 25_000);
});

test("empty branch name is refused", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  await tui.sendKey("n");
  await tui.waitForText("New worktree");
  await tui.sendKey("Enter"); // only the "feature/" prefix
  await tui.waitForText("Enter a branch name");
  await tui.sendKey("Escape");
  await tui.waitForGone("New worktree");
});
