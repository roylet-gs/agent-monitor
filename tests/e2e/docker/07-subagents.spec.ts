import { test, expect } from "@playwright/test";
import { RustTui, resetMock } from "./helpers/rust-tui";

// e2e/entrypoint-rust.sh seeds main through `am-rs hook-event`:
// UserPromptSubmit → SubagentStart (Explore, agent a1b2c3d4) → PreToolUse Grep
// → Stop while the subagent still runs, which leaves main "delegating". The
// running subagent shows as a `◆ 1 subagent` badge after the row's name.

test.beforeEach(async () => {
  await resetMock();
});

test("delegating worktree shows its running-subagent badge", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();

  // ◆ status glyph (delegating) + the badge on main's row.
  await tui.waitForMatch(/◆ main\b.*◆ 1 subagent\s/);

  await tui.selectRow("main");
  expect(await tui.selectedRow()).toMatch(/◆ main\b.*◆ 1 subagent\s/);
  await tui.screenshot("subagents");
});

test("worktrees without running subagents have no badge", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  await tui.selectRow("feature/to-delete");
  const row = await tui.selectedRow();
  expect(row).toContain("feature/to-delete");
  expect(row).not.toContain("subagent");
  // Only main carries a badge.
  const badged = (await tui.lines()).filter((l) => l.includes("subagent"));
  expect(badged).toHaveLength(1);
  expect(badged[0]).toContain(" main ");
});
