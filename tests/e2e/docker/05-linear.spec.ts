import { test, expect } from "@playwright/test";
import { RustTui, resetMock } from "./helpers/rust-tui";

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
  await tui.screenshot("linear-ticket");
});

/** Open Settings → Sort order (8th field) in the sort editor. */
async function openSortEditor(tui: RustTui) {
  await tui.sendKey("s");
  await tui.waitForText("esc close");
  await tui.press("j", 7);
  await tui.waitForText("Criteria applied in order; first difference wins");
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
  // Headers never take a selection index: still "N of 3" worktree rows.
  expect(await tui.getScreenText()).toMatch(/ \d of 3 /);
  await tui.screenshot("linear-project");

  // Restore the default order (disable it, move it back down).
  await openSortEditor(tui);
  await tui.sendKey(" ");
  await tui.press("J", 6);
  await tui.sendKey("Enter");
  await tui.sendKey("Escape");
  await tui.waitForGone("═ Dashboard Revamp ═", 10_000);
});
