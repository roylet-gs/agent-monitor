import { test, expect } from "@playwright/test";
import { RustTui, resetMock, setupMock } from "./helpers/rust-tui";

test.beforeEach(async () => {
  await resetMock();
});

const HELP = "Dense layout for small panes";

async function setCompact(tui: RustTui, on: boolean): Promise<void> {
  await tui.openSetting(HELP);
  const value = /Compact view\s+(on|off)/.exec(await tui.getScreenText())?.[1];
  if (value !== (on ? "on" : "off")) await tui.sendKey(" ");
  await tui.waitForMatch(new RegExp(`Compact view\\s+${on ? "on" : "off"}`));
  await tui.sendKey("Escape");
  await tui.waitForGone("esc close");
}

test("compact view: top lines only, jobs pane and split reviews open below", async ({ page }) => {
  await setupMock({ reviews: true });
  const tui = new RustTui(page);
  await tui.open();
  await setCompact(tui, true);

  // The list keeps only its top line, which also carries `N of M`.
  let lines = await tui.lines();
  expect(lines[0]).toMatch(/^─Worktrees .* \d+ of \d+/);
  // No side or bottom borders (the key bar's `│` separators aside).
  expect(lines.slice(0, -1).some((l) => l.startsWith("│") || l.startsWith("╰"))).toBe(false);
  await tui.screenshot("compact-dashboard");

  // A create's jobs pane sits under the list, full width.
  const suffix = `e2e-compact-${Date.now() % 1_000_000}`;
  const branch = `feature/${suffix}`;
  await tui.sendKey("n");
  await tui.waitForText("New worktree");
  await tui.type(suffix);
  await tui.waitForGone("checking…", 10_000);
  await tui.sendKey("Enter");
  await tui.waitForText("Run the repo's startup script?", 30_000);
  lines = await tui.lines();
  const list = lines.findIndex((l) => l.startsWith("─Worktrees"));
  const jobs = lines.findIndex((l) => l.startsWith("─Jobs"));
  expect(list).toBe(0);
  expect(jobs).toBeGreaterThan(list);
  await tui.screenshot("compact-jobs-below");
  await tui.skipStartupScript();

  // `V` stacks the review page under the list even on a wide window.
  await tui.sendKey("V");
  await tui.waitForText("─Review requests");
  lines = await tui.lines();
  expect(lines[0]).not.toContain("Review requests");
  expect(lines.findIndex((l) => l.startsWith("─Review requests"))).toBeGreaterThan(0);
  await tui.screenshot("compact-review-split");
  await tui.sendKey("Escape");
  await tui.waitForGone("─Review requests");

  // Restore: off brings the borders back; then remove the worktree.
  await setCompact(tui, false);
  await tui.waitForMatch(/╰/);
  await tui.selectRow(branch);
  await tui.sendKey("d");
  await tui.waitForText(`Delete worktree "${branch}"`);
  await tui.sendKey("y");
  await tui.waitForText(`Also delete local branch ${branch}?`);
  await tui.sendKey("y");
  await tui.waitForGone(branch, 20_000);
});
