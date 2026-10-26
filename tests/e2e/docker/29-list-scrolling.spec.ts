import { test, expect } from "@playwright/test";
import { RustTui, resetMock } from "./helpers/rust-tui";

// The seeded list is short, so a short window makes it overflow: ttyd fits
// the terminal to the page, giving a `─Worktrees` pane only a few rows tall.
test.use({ viewport: { width: 1280, height: 180 } });

test.beforeEach(async () => {
  await resetMock();
});

/** Rows inside the `─Worktrees` pane, between its top and bottom borders. */
async function paneRows(tui: RustTui): Promise<string[]> {
  const lines = await tui.lines();
  const top = lines.findIndex((l) => l.includes("─Worktrees"));
  const bottom = lines.findIndex((l, i) => i > top && /^[└╰]/.test(l));
  return lines.slice(top + 1, bottom);
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
