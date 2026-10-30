import { test, expect } from "@playwright/test";
import { RustTui, resetMock } from "./helpers/rust-tui";

// The seeded list is short, so a short window makes it overflow: ttyd fits
// the terminal to the page, giving a `─Worktrees` pane only a few rows tall.
test.use({ viewport: { width: 1280, height: 180 } });

test.beforeEach(async () => {
  await resetMock();
});

/**
 * Rows inside the `· Worktrees` pane, between its top and bottom borders, with
 * ages under a minute (`13s`) masked: they tick between two looks at the same row.
 */
async function paneRows(tui: RustTui): Promise<string[]> {
  const lines = await tui.lines();
  const top = lines.findIndex((l) => l.includes("· Worktrees"));
  const bottom = lines.findIndex((l, i) => i > top && /^[└╰]/.test(l));
  return lines.slice(top + 1, bottom).map((l) => l.replace(/\b\d{1,2}s\b/g, "Ns"));
}

/**
 * A worktree row: border, selection bar or space, any stack-tree prefix,
 * then a status glyph and the name. Headers (`│work ───`, `│ — ENG-123 … —`),
 * blank lines and session rows (`├ ● claude …`) don't match.
 */
const WORKTREE_ROW = /^│[▌ ](?:[│├└]─ |│  | {3})*[^\s│├└─—] \S/;

test("the list scrolls only when the selection leaves the view", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  await tui.waitForText("feature/to-delete");

  // Top of the list: remember the full first page.
  await tui.sendKey("Home");
  await tui.waitForMatch(/ 1 of \d+ /);
  const firstPage = await paneRows(tui);

  // Bottom: the page must have scrolled, i.e. the list really overflows.
  await tui.sendKey("G");
  await tui.waitForMatch(/ (\d+) of \1 /);
  const bottomPage = await paneRows(tui);
  expect(bottomPage[0], `list doesn't overflow:\n${await tui.getScreenText()}`).not.toBe(firstPage[0]);
  const selIndex = () => paneRows(tui).then((r) => r.findIndex((l) => l.startsWith("│▌")));
  // The selection sits on the bottom row of the pane.
  expect(await selIndex()).toBe(bottomPage.length - 1);
  await tui.screenshot("list-scroll-bottom");

  // ↑ once: the cursor moves up the page, the page stays put.
  const bottomName = await tui.selectedName();
  await tui.sendKey("ArrowUp");
  expect(await tui.selectedName()).not.toBe(bottomName);
  expect((await paneRows(tui))[0]).toBe(bottomPage[0]);
  expect(await selIndex()).toBeLessThan(bottomPage.length - 1);
  await tui.screenshot("list-scroll-after-up");

  // Keep going up while a worktree row is still above the selection on
  // this page: the page never moves.
  for (let guard = 0; guard < 20; guard++) {
    const rows = await paneRows(tui);
    const sel = rows.findIndex((l) => l.startsWith("│▌"));
    if (!rows.slice(0, sel).some((l) => WORKTREE_ROW.test(l))) break;
    await tui.sendKey("ArrowUp");
    expect((await paneRows(tui))[0]).toBe(bottomPage[0]);
  }
  // The selection is now the top worktree row on the page; one more ↑
  // scrolls the page to bring the row above into view.
  const atTop = await tui.selectedName();
  await tui.sendKey("ArrowUp");
  expect(await tui.selectedName()).not.toBe(atTop);
  const scrolled = await paneRows(tui);
  expect(scrolled[0]).not.toBe(bottomPage[0]);
  expect(scrolled.some((l) => l.startsWith("│▌"))).toBe(true);
  await tui.screenshot("list-scroll-past-top");
});

/**
 * Rows inside the Settings box, cut to its left part (label + value), so
 * whatever the worktree pane shows beside it doesn't count.
 */
async function settingsRows(tui: RustTui): Promise<string[]> {
  const lines = await tui.lines();
  const top = lines.findIndex((l) => l.includes("─Settings"));
  const col = lines[top].indexOf("╭─Settings");
  const bottom = lines.findIndex((l, i) => i > top && l[col] === "╰");
  return lines.slice(top + 1, bottom).map((l) => l.slice(col, col + 40));
}

/** The settings page with the selection bar removed, to tell whether it scrolled. */
const pageText = (rows: string[]) => rows.map((l) => l.replace("│▌", "│ ")).join("\n");

/** A settings field row (not a section header, blank line or banner). */
const FIELD_ROW = /^│[▌ ] {2}\S/;

test("settings scroll like the worktree list", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  await tui.sendKey("s");
  await tui.waitForText("─Settings");

  // End: the last field sits on the box's bottom field row.
  await tui.sendKey("End");
  await tui.waitForText("Reset settings…");
  const bottomPage = await settingsRows(tui);
  expect(bottomPage.join("\n"), `settings don't overflow:\n${await tui.getScreenText()}`).not.toMatch(/│ General/);
  const selIndex = (rows: string[]) => rows.findIndex((l) => l.startsWith("│▌"));
  expect(bottomPage[selIndex(bottomPage)]).toContain("Reset settings…");
  await tui.screenshot("settings-scroll-bottom");

  // ↑ (and k) move up the page without scrolling it while a field is above.
  for (let guard = 0; guard < 40; guard++) {
    const rows = await settingsRows(tui);
    if (!rows.slice(0, selIndex(rows)).some((l) => FIELD_ROW.test(l))) break;
    await tui.sendKey(guard % 2 ? "k" : "ArrowUp");
    expect(pageText(await settingsRows(tui))).toBe(pageText(bottomPage));
  }
  await tui.screenshot("settings-scroll-at-top");
  // The selection is the top field on the page; one more ↑ scrolls.
  await tui.sendKey("ArrowUp");
  const scrolled = await settingsRows(tui);
  expect(pageText(scrolled)).not.toBe(pageText(bottomPage));
  expect(selIndex(scrolled)).toBeGreaterThanOrEqual(0);
  await tui.screenshot("settings-scroll-past-top");

  // Home goes back to the first field.
  await tui.sendKey("Home");
  await tui.waitForMatch(/│ General/);
  expect((await settingsRows(tui)).find((l) => l.startsWith("│▌"))).toContain("Mode");
  await tui.sendKey("Escape");
  await tui.waitForGone("─Settings");
});
