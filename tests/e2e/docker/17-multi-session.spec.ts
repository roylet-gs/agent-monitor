import { test, expect } from "@playwright/test";
import { RustTui, resetMock } from "./helpers/rust-tui";

// e2e/entrypoint.sh runs two agents at once in feature/pair through the real
// hook path: a Claude session (executing, one running Explore subagent, a
// 300k transcript) and a Gemini session (turn finished → done). Each keeps
// its own status, listed under the worktree row.

test.beforeEach(async () => {
  await resetMock();
});

test("two agents in one worktree are listed under its row with their own status", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();

  // The worktree row: headline glyph (executing beats done) + session count.
  await tui.waitForMatch(/● feature\/pair\b.*⧉ 2 sessions/);
  const lines = await tui.lines();
  const at = lines.findIndex((l) => l.includes("feature/pair"));
  const parent = lines[at];
  expect(parent).not.toContain("◆");
  expect(parent).not.toMatch(/\d+k │/);

  // Then one row per session, oldest first: tree connector, own glyph,
  // harness, status, own subagent badge and context bar.
  const claude = lines[at + 1];
  expect(claude).toMatch(/├ ● claude\s+executing/);
  expect(claude).toContain("◆ 1 subagent");
  expect(claude).toMatch(/300k │/);
  const gemini = lines[at + 2];
  expect(gemini).toMatch(/└ ✓ gemini\s+done/);
  expect(gemini).not.toContain("◆");
  expect(gemini).not.toMatch(/\d+k │/);
  await tui.screenshot("multi-session");

  // Session rows take no selection index: j from the worktree skips them.
  await tui.selectRow("feature/pair");
  await tui.sendKey("j");
  expect(await tui.selectedName()).toBe("feature/to-delete");

  // Clicking a session row selects its worktree.
  await tui.clickText("gemini  done");
  expect(await tui.selectedName()).toBe("feature/pair");
  await tui.screenshot("multi-session-selected");
});
