import { test, expect } from "@playwright/test";
import { RustTui, resetMock, setupMock } from "./helpers/rust-tui";

// The default linear-issue fixture links every branch to ENG-123 in project
// "Dashboard Revamp" (the mock answers each aliased issueVcsBranchSearch).

test.beforeEach(async () => {
  await resetMock();
});

test("Linear ticket column and ticket cluster header", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  await tui.waitForText("ENG-123", 15_000);
  await tui.selectRow("main");
  // The identifier sits in the row's ticket column.
  await tui.waitForMatch(/│▌.*\sENG-123\s/, 5_000);
  // Adjacent worktrees with the same ticket get a "— ticket —" header.
  await tui.waitForText("— ENG-123 Implement dashboard feature —");
  // The cluster opens the repo section: no blank line between the repo header
  // and the ticket header.
  const lines = await tui.lines();
  const header = lines.findIndex((l) => l.includes("— ENG-123 Implement dashboard feature —"));
  expect(lines[header - 1]).toMatch(/work ─/);
  await tui.screenshot("linear-ticket");
});

test("blank line above a ticket cluster after another row", async ({ page }) => {
  // feature/pair gets its own ticket (ENG-100). The default sort (non-main
  // first, ticket ascending) gives feature/pair alone, then feature/to-delete +
  // main under one ENG-123 header, which is set off from feature/pair (and its
  // session rows) by a blank line above it.
  const own = {
    identifier: "ENG-100",
    title: "Tidy settings page",
    url: "https://linear.app/team/issue/ENG-100",
    state: { name: "Todo", color: "#94a3b8", type: "unstarted" },
    priorityLabel: "Medium",
    assignee: { name: "Test User" },
    project: null,
    attachments: { nodes: [] },
  };
  await setupMock({ linearByBranch: { "feature/pair": own } });
  const tui = new RustTui(page);
  await tui.open();
  // Wait until feature/pair carries its own ticket (the cache may hydrate ENG-123 first).
  await tui.waitForMatch(/^│▌?\s*\S+ feature\/pair\b.*ENG-100/m, 15_000);
  await tui.waitForText("— ENG-123 Implement dashboard feature —", 15_000);

  const lines = await tui.lines();
  const header = lines.findIndex((l) => l.includes("— ENG-123 Implement dashboard feature —"));
  const pairRow = lines.findIndex((l) => /^│▌?\s*\S+ feature\/pair\b/.test(l));
  expect(pairRow).toBeGreaterThanOrEqual(0);
  expect(header).toBeGreaterThan(pairRow + 1);
  // No header for a ticket only one row has.
  expect(lines.join("\n")).not.toContain("— ENG-100");
  // The line right above the ticket header is blank (borders + spaces)…
  expect(lines[header - 1]).toMatch(/^│\s*│?\s*$/);
  // …and the one above that is feature/pair's block (its last session row), not a header.
  expect(lines[header - 2]).not.toMatch(/^│\s*│?\s*$/);
  expect(lines[header - 2]).not.toMatch(/ ─{3}|═|— ENG/);
  // The cluster follows the header: feature/to-delete, then main.
  expect(lines[header + 1]).toContain("feature/to-delete");
  expect(lines.findIndex((l, i) => i > header && /^│▌?\s*\S main\b.*ENG-123/.test(l))).toBeGreaterThan(header + 1);
  await tui.screenshot("linear-blank-line-above-cluster");

  // Restore the default cache (every worktree on ENG-123) for later specs.
  await resetMock();
  await tui.sendKey("r");
  await tui.waitForMatch(/^│▌?\s*\S+ feature\/pair\b.*ENG-123/m, 15_000);
});

test("blank line after a ticket cluster before the next row", async ({ page }) => {
  // Only main has no ticket. Default sort (non-main first, ticket first, newest
  // first) gives feature/pair + feature/to-delete under one ENG-123 header,
  // then main, which is set off by a blank line.
  await setupMock({ linearByBranch: { main: null } });
  const tui = new RustTui(page);
  await tui.open();
  await tui.waitForText("— ENG-123 Implement dashboard feature —", 15_000);
  // Wait until main has lost its ticket (the cache may hydrate it first).
  await tui.waitForMatch(/^│▌?\s*\S main\b(?!.*ENG-123).*$/m, 15_000);

  const lines = await tui.lines();
  const header = lines.findIndex((l) => l.includes("— ENG-123 Implement dashboard feature —"));
  expect(header).toBeGreaterThanOrEqual(0);
  const mainRow = lines.findIndex((l, i) => i > header && /^│▌?\s*\S main\b/.test(l));
  expect(mainRow).toBeGreaterThan(header + 2);
  // The row right before main is a blank line inside the pane (borders + spaces).
  expect(lines[mainRow - 1]).toMatch(/^│\s*│?\s*$/);
  // …and the row before that is the cluster's last worktree, with the ticket.
  expect(lines[mainRow - 2]).toContain("feature/to-delete");
  expect(lines[mainRow - 2]).toContain("ENG-123");
  expect(lines[mainRow]).not.toContain("ENG-123");
  await tui.screenshot("ticket-cluster-blank-line");

  // Restore: give main its ticket back and refresh, so later specs see the
  // default cache (one cluster with every worktree).
  await resetMock();
  await tui.sendKey("r");
  await tui.waitForMatch(/^│▌?\s*\S main\b.*ENG-123/m, 15_000);
});

/** Open Settings → Sort order in the sort editor. */
async function openSortEditor(tui: RustTui) {
  await tui.openSetting("Criteria applied in order; first difference wins");
  await tui.sendKey("Enter");
  await tui.waitForText("Sort order — space on/off");
}

test("project headers when Linear project is the top sort key", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  await tui.waitForText("ENG-123", 15_000);
  expect(await tui.getScreenText()).not.toContain("═ Dashboard Revamp ═");

  // Default order: … 7. Linear project (off). Move it to the top and enable it.
  await openSortEditor(tui);
  await tui.press("j", 6);
  await tui.press("K", 6);
  await tui.waitForMatch(/▌\s*1\. \[ \] /);
  await tui.sendKey(" ");
  await tui.waitForMatch(/▌\s*1\. \[x\] /);
  await tui.screenshot("sort-editor");
  await tui.sendKey("Enter");
  await tui.sendKey("Escape");
  await tui.waitForGone("esc close");

  await tui.waitForText("═ Dashboard Revamp ═", 10_000);
  // Headers and session rows never take a selection index: still "N of 6"
  // rows (3 worktrees + 3 standalone sessions: Claude, Gemini, Codex).
  expect(await tui.getScreenText()).toMatch(/ \d of 6 /);
  await tui.screenshot("linear-project");

  // The standalone sessions under "Other sessions" have no Linear project:
  // they get a blank spacer line but no "═ No project ═" header.
  const projectSorted = await tui.getScreenText();
  expect(projectSorted).toContain("Other sessions");
  expect(projectSorted).not.toContain("No project");
  await tui.screenshot("linear-project-sort-no-noproject-header");

  // Restore the default order (disable it, move it back down).
  await openSortEditor(tui);
  await tui.sendKey(" ");
  await tui.press("J", 6);
  await tui.sendKey("Enter");
  await tui.sendKey("Escape");
  await tui.waitForGone("═ Dashboard Revamp ═", 10_000);
});
