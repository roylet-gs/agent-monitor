import { test, expect } from "@playwright/test";
import { RustTui, resetMock, setupMock } from "./helpers/rust-tui";

// Folded stacks are remembered across restarts (`collapsedStacks` in
// settings.json). Same stack as 36-stack-grouping: feature/pair's open PR
// targets feature/to-delete, so it stacks under it.

const pr = (number: number, baseRefName: string) => ({
  number,
  title: `feat: stacked change ${number}`,
  url: `https://github.com/test/repo/pull/${number}`,
  state: "OPEN",
  isDraft: false,
  reviewDecision: "REVIEW_REQUIRED",
  baseRefName,
  statusCheckRollup: [{ status: "COMPLETED", conclusion: "SUCCESS", name: "build" }],
});

const STACK = {
  ghByBranch: {
    "feature/to-delete": null,
    "feature/pair": pr(52, "feature/to-delete"),
  },
};

const STACKED_PAIR = /^│[▌ ] {2}[│ ]*[├└]─ ?\S feature\/pair\b/m;
const OPEN_ROOT = /^│[▌ ]▾ \S feature\/to-delete\b/m;
const FOLDED_ROOT = /^│[▌ ]▸ \S feature\/to-delete\b.* \+1 stacked hidden /m;

test.beforeEach(async () => {
  await resetMock();
});

test("a folded stack stays folded after a restart", async ({ page }) => {
  await setupMock(STACK);
  const tui = new RustTui(page);
  await tui.open();
  // The integration cache may hydrate the default PR first; `r` refetches.
  await tui.sendKey("r");
  await tui.waitForMatch(STACKED_PAIR, 20_000);
  expect(await tui.getScreenText()).toMatch(OPEN_ROOT);

  try {
    // ← on the stacked branch folds the stack to its root.
    await tui.selectRow("feature/pair");
    await tui.sendKey("ArrowLeft");
    await tui.waitForMatch(FOLDED_ROOT);
    await tui.waitForGone("feature/pair");

    // Restart am-tui (ttyd spawns a new process per connection).
    await page.reload();
    await tui.open();
    await tui.waitForMatch(FOLDED_ROOT, 20_000);
    expect(await tui.getScreenText()).not.toContain("feature/pair");
    await tui.screenshot("fold-persists-after-restart");

    // → unfolds it, and that is remembered too.
    await tui.selectRow("feature/to-delete");
    await tui.sendKey("ArrowRight");
    await tui.waitForMatch(STACKED_PAIR);
    await tui.waitForGone("stacked hidden");

    await page.reload();
    await tui.open();
    await tui.waitForMatch(STACKED_PAIR, 20_000);
    expect(await tui.getScreenText()).toMatch(OPEN_ROOT);
    expect(await tui.getScreenText()).not.toContain("stacked hidden");
    await tui.screenshot("fold-persists-unfolded");
  } finally {
    // Never leave the stack folded for later specs.
    const screen = await tui.getScreenText();
    if (screen.includes("stacked hidden")) {
      await tui.selectRow("feature/to-delete");
      await tui.sendKey("ArrowRight");
      await tui.waitForGone("stacked hidden");
    }
  }

  // Back to the default fixtures (no stack) in the cache for later specs.
  await resetMock();
  await tui.sendKey("r");
  await tui.waitForGone("▾", 20_000);
});
