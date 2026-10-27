import { test, expect } from "@playwright/test";
import { RustTui, resetMock } from "./helpers/rust-tui";

test.beforeEach(async () => {
  await resetMock();
});

test("dashboard renders the worktree pane and key bar with the seeded repo", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  await tui.waitForText("feature/to-delete");

  const text = await tui.getScreenText();
  expect(text).toContain("─Worktrees");
  expect(text).toMatch(/ \d of \d+ /);
  // The Subagents pane and the mode pill are gone.
  expect(text).not.toContain("Subagents");
  expect(text).not.toContain(" WORKTREES ");
  // Repo header (two groups: the repo + "Other sessions"): name + rule.
  expect(text).toMatch(/│work ─+/);
  expect(text).toContain("main");
  // The last-commit age column (git runs on its own thread, so it fills in
  // a moment after the first frame): the seeded commit is minutes old.
  await tui.waitForMatch(/ main [^\n]*\s\d{1,2}[smhdw] /);
  await tui.waitForMatch(/feature\/to-delete[^\n]*\s\d{1,2}[smhdw] /);
  const keys = await tui.keyBar();
  expect(keys).toContain("⏎ open");
  expect(keys).toContain("g PR");
  expect(keys).toContain("h help");
  expect(keys).toContain("q quit");
  // The `t` terminal and `c` agent actions are gone.
  expect(keys).not.toContain("t terminal");
  expect(keys).not.toContain("c agent");

  await tui.screenshot("dashboard");
  await tui.screenshot("dashboard-keybar");
});

test("c is no longer bound: no popup or notice, dashboard stays", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  await tui.selectRow("feature/to-delete");
  const before = await tui.keyBar();

  for (const key of ["c"]) {
    await tui.sendKey(key);
    await tui.waitForText("─Worktrees");
    const text = await tui.getScreenText();
    expect(text).not.toContain("Copied");
    expect(text).not.toMatch(/terminal|resum/i);
    expect(await tui.keyBar()).toBe(before);
    expect(await tui.selectedName()).toBe("feature/to-delete");
  }
});

test("j/k move the ▌ selection; Home/G jump to top/bottom", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();

  await tui.selectRow("feature/to-delete");
  expect(await tui.selectedName()).toBe("feature/to-delete");
  await tui.sendKey("j");
  expect(await tui.selectedName()).toBe("main");
  await tui.sendKey("k");
  expect(await tui.selectedName()).toBe("feature/to-delete");
  // Exactly one row is marked.
  expect((await tui.lines()).filter((l) => l.includes("│▌"))).toHaveLength(1);

  await tui.sendKey("G");
  await tui.waitForMatch(/ (\d+) of \1 /);
  await tui.sendKey("Home");
  await tui.waitForMatch(/ 1 of \d+ /);
});

test("w toggles the log pane", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  await tui.sendKey("w");
  await tui.waitForText("[w] Log");
  await tui.sendKey("w");
  await tui.waitForGone("[w] Log");
});
