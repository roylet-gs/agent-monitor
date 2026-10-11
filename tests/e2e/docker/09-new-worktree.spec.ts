import { test, expect } from "@playwright/test";
import { RustTui, resetMock } from "./helpers/rust-tui";

// The container's repo has a local bare `origin` with a branch that exists
// only there: feature/on-origin (see e2e/entrypoint-rust.sh).

test.beforeEach(async () => {
  await resetMock();
});

test("live check flags a branch that exists on origin; Enter offers to pull it", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();

  await tui.sendKey("n");
  await tui.waitForText("New worktree");
  await tui.waitForText("tab next  ⏎ create  esc cancel");
  // Branch is prefilled with the default prefix.
  await tui.waitForMatch(/Branch\s+feature\/▏/);

  await tui.type("on-origin");
  await tui.waitForText("✓ exists on origin — Enter will offer to pull it", 10_000);
  await tui.waitForText("Base      tracks origin/feature/on-origin");
  await tui.screenshot("new-worktree-on-origin");

  await tui.sendKey("Enter");
  await tui.waitForText('Branch "feature/on-origin" exists on origin', 15_000);
  await tui.waitForText("Pull remote (track origin/feature/on-origin)");
  await tui.screenshot("branch-exists");
  await tui.sendKey("Escape");
  await tui.waitForGone("Branch exists");
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
  await tui.waitForGone("Creating ", 30_000);
  await tui.waitForText(`feature/${name}`, 15_000);
  // pending_select puts the new row under the cursor.
  await tui.waitForMatch(new RegExp(`│▌[│├└─ ]*\\S feature/${name}\\s`), 10_000);
  expect(await tui.selectedName()).toBe(`feature/${name}`);
  await tui.screenshot("worktree-created");
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
