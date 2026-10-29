import { test, expect } from "@playwright/test";
import { RustTui, resetMock } from "./helpers/rust-tui";

// `f` opens the Worktree filters box (text, agent status, PR, git); `F` turns
// those off and back on, while the base `hide*` filters (Settings → Base
// filters) always apply. The seed sets `hideMainBranch: false`, which the
// border names as `main shown`.

test.beforeEach(async () => {
  await resetMock();
});

const worktreeRows = async (tui: RustTui) =>
  (await tui.lines()).filter((l) => /feature\/to-delete|feature\/pair|\bmain\b/.test(l) && /[◑◐●✓·○]/.test(l));

test("f filters the worktree list on top of the base filters and F turns only those off and on", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  await tui.waitForText("feature/to-delete");

  // The box, starting on its master switch.
  await tui.sendKey("f");
  await tui.waitForText("Worktree filters");
  await tui.waitForText("Text search");
  await tui.screenshot("worktree-filters-box");

  // Text search: only feature/pair is left.
  await tui.sendKey("j");
  await tui.sendKey("Enter");
  await tui.type("pair");
  await tui.sendKey("Enter");
  await tui.sendKey("Escape");
  await tui.waitForGone("Worktree filters");
  await tui.waitForGone("feature/to-delete");
  await tui.waitForMatch(/"pair" · main shown · \d+ hidden/);
  expect((await tui.lines()).some((l) => l.includes("feature/pair"))).toBe(true);
  await tui.screenshot("worktree-filters-text");

  // F: every row back, and the border says the filters are off (the base
  // filters still listed).
  await tui.sendKey("F");
  await tui.waitForText("filters off · F · main shown");
  await tui.waitForText("feature/to-delete");
  await tui.screenshot("worktree-filters-off");
  await tui.sendKey("F");
  await tui.waitForGone("feature/to-delete");
  await tui.waitForGone("filters off");

  // Clear the text, then show only rows with a working agent: main (delegating)
  // and feature/pair (executing) stay, feature/to-delete (no agent) goes.
  await tui.sendKey("f");
  await tui.waitForText("Worktree filters");
  await tui.sendKey("j");
  await tui.sendKey("Enter");
  await tui.press("Backspace", 4);
  await tui.sendKey("Enter");
  await tui.sendKey("j");
  await tui.press("ArrowRight", 2);
  await tui.waitForText("active");
  await tui.sendKey("Escape");
  await tui.waitForGone("Worktree filters");
  await tui.waitForMatch(/active agents · main shown · \d+ hidden/);
  const rows = (await worktreeRows(tui)).join("\n");
  expect(rows).toContain("feature/pair");
  expect(rows).not.toContain("feature/to-delete");
  await tui.screenshot("worktree-filters-status");

  // Reset filters… asks, then clears the f filters; the box has no base
  // filters, and the reset leaves the seeded hideMainBranch: false alone.
  await tui.sendKey("f");
  await tui.waitForText("Worktree filters");
  expect((await tui.lines()).some((l) => l.includes("Hide main branch"))).toBe(false);
  await tui.sendKey("End");
  await tui.sendKey("Enter");
  await tui.waitForText("Reset the worktree filters to their defaults?");
  await tui.sendKey("y");
  await tui.waitForGone("Reset the worktree filters");
  await tui.sendKey("Escape");
  await tui.waitForGone("Worktree filters");
  await tui.waitForText("feature/to-delete");
  await tui.waitForText("main shown");
  await tui.waitForGone("active agents");
});
