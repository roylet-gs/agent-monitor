import { test, expect } from "@playwright/test";
import { RustTui, resetMock, setupMock } from "./helpers/rust-tui";

// Stacks from PR bases: feature/pair's open PR targets feature/to-delete (so it
// stacks under it), and feature/to-delete's targets feature/epic-base, a branch
// with no worktree (the faint `┌ feature/epic-base · no worktree` line). Both
// tickets share the parent issue ENG-100, so the stack sits under
// `— ENG-100 Epic one —` (`groupStacksByTicket`, on by default).

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

const ticket = (identifier: string, title: string) => ({
  identifier,
  title,
  url: `https://linear.app/team/issue/${identifier}`,
  state: { name: "In Progress", color: "#0ea5e9", type: "started" },
  priorityLabel: "High",
  assignee: { name: "Test User" },
  project: null,
  parent: { identifier: "ENG-100", title: "Epic one" },
  attachments: { nodes: [] },
});

const HEADER = "— ENG-100 Epic one —";
const BASE = "┌ feature/epic-base · no worktree";
/**
 * A worktree row (selected or not): bar, the 2-column fold gutter (`▾ `/`▸ `
 * on a stack root, else blank), any tree prefix, status glyph, name.
 */
const row = (branch: string) => new RegExp(`^│[▌ ][▾▸ ] [│├└─ ]*\\S ${branch.replace(/[/.]/g, "\\$&")}\\b`);
const STACKED_PAIR = /^│[▌ ] {2}[│ ]*[├└]─ ?\S feature\/pair\b/m;
const OPEN_ROOT = /^│[▌ ]▾ \S feature\/to-delete\b/m;
const FOLDED_ROOT = /^│[▌ ]▸ \S feature\/to-delete\b.* \+1 stacked hidden /m;

const GROUP_HELP = "Stacks sit under the Linear ticket they belong to";
const GROUP_ROW = /Group stacks by ticket\s+(on|off)/;

test.beforeEach(async () => {
  await resetMock();
});

/** Settings → "Group stacks by ticket": flip it, check the new value, close. */
async function toggleGroupByTicket(tui: RustTui, expected: "on" | "off") {
  await tui.openSetting(GROUP_HELP);
  await tui.sendKey(" ");
  await tui.waitForMatch(new RegExp(`Group stacks by ticket\\s+${expected}`));
  await tui.sendKey("Escape");
  await tui.waitForGone("esc close");
}

test("stacks group under their parent ticket, show an unpulled base and fold with ←/→", async ({ page }) => {
  await setupMock({
    ghByBranch: {
      "feature/to-delete": pr(51, "feature/epic-base"),
      "feature/pair": pr(52, "feature/to-delete"),
    },
    linearByBranch: {
      "feature/to-delete": ticket("ENG-201", "Base of the epic"),
      "feature/pair": ticket("ENG-202", "On top of the base"),
    },
  });
  const tui = new RustTui(page);
  await tui.open();
  // The integration cache may hydrate the default PR/ticket first; `r` refetches.
  await tui.sendKey("r");
  await tui.waitForMatch(STACKED_PAIR, 20_000);
  await tui.waitForText(BASE, 15_000);
  await tui.waitForText(HEADER, 15_000);

  const lines = await tui.lines();
  const header = lines.findIndex((l) => l.includes(HEADER));
  const base = lines.findIndex((l) => l.includes(BASE));
  const root = lines.findIndex((l) => row("feature/to-delete").test(l));
  const pair = lines.findIndex((l) => STACKED_PAIR.test(l));
  expect(header).toBeGreaterThanOrEqual(0);
  expect(base).toBeGreaterThan(header);
  // The base line sits directly above the stack's root…
  expect(root).toBe(base + 1);
  // …and feature/pair follows it with a tree prefix.
  expect(pair).toBeGreaterThan(root);
  // The root has the open fold arrow and no tree prefix; other rows get a
  // blank gutter so names stay aligned.
  expect(lines[root]).toMatch(OPEN_ROOT);
  expect(lines[root]).not.toMatch(/[├└]─/);
  expect(lines.find((l) => row("main").test(l))).toMatch(/^│[▌ ] {2}\S main\b/);
  await tui.screenshot("stack-grouping");

  // ← inside the stack (on feature/pair) folds the whole stack and selects the root.
  await tui.selectRow("feature/pair");
  await tui.sendKey("ArrowLeft");
  await tui.waitForMatch(FOLDED_ROOT);
  await tui.waitForGone("feature/pair");
  expect(await tui.selectedName()).toBe("feature/to-delete");
  expect(await tui.selectedRow()).toContain("+1 stacked hidden");
  await tui.screenshot("stack-folded");

  // → on the folded root unfolds it (selection stays on the root)…
  await tui.sendKey("ArrowRight");
  await tui.waitForMatch(STACKED_PAIR);
  await tui.waitForGone("stacked hidden");
  expect(await tui.getScreenText()).toMatch(OPEN_ROOT);
  expect(await tui.selectedName()).toBe("feature/to-delete");
  // …→ on the open root moves to its first stacked branch, → there does nothing.
  await tui.sendKey("ArrowRight");
  await tui.waitForMatch(/^│▌ {2}[│ ]*[├└]─ ?\S feature\/pair\b/m);
  await tui.sendKey("ArrowRight");
  expect(await tui.selectedName()).toBe("feature/pair");
  expect(await tui.getScreenText()).toMatch(STACKED_PAIR);
  // ← on the root folds it too.
  await tui.selectRow("feature/to-delete");
  await tui.sendKey("ArrowLeft");
  await tui.waitForMatch(FOLDED_ROOT);
  await tui.sendKey("ArrowRight");
  await tui.waitForMatch(STACKED_PAIR);

  // Mouse: a click on the `▾` arrow folds, a click on the pill unfolds.
  await tui.clickText("▾");
  await tui.waitForMatch(FOLDED_ROOT);
  await tui.waitForGone("feature/pair");
  await tui.screenshot("stack-folded-by-click");
  await tui.clickText("stacked hidden");
  await tui.waitForMatch(STACKED_PAIR);
  await tui.waitForGone("stacked hidden");
  expect(await tui.getScreenText()).toMatch(OPEN_ROOT);

  // `z` folds every stack, and with all folded unfolds them all.
  await tui.sendKey("z");
  await tui.waitForMatch(FOLDED_ROOT);
  await tui.waitForText("Folded 1 stack");
  await tui.sendKey("z");
  await tui.waitForMatch(STACKED_PAIR);
  await tui.waitForText("Unfolded 1 stack");

  // Settings → Group stacks by ticket off: no ticket header (the stack stays).
  await toggleGroupByTicket(tui, "off");
  await tui.waitForGone(HEADER);
  expect(await tui.getScreenText()).toMatch(STACKED_PAIR);
  expect(await tui.getScreenText()).not.toContain("— ENG-");

  // Restore it.
  await toggleGroupByTicket(tui, "on");
  await tui.waitForText(HEADER);
  await tui.openSetting(GROUP_HELP);
  expect(GROUP_ROW.exec(await tui.getScreenText())?.[1]).toBe("on");
  await tui.sendKey("Escape");
  await tui.waitForGone("esc close");

  // Back to the default fixtures (no stack) in the cache for later specs.
  await resetMock();
  await tui.sendKey("r");
  await tui.waitForGone(BASE, 20_000);
  await tui.waitForGone(HEADER, 15_000);
});
