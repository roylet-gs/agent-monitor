import { test, expect } from "@playwright/test";
import { RustTui, resetMock } from "./helpers/rust-tui";

// e2e/entrypoint.sh runs three agents at once in feature/pair through the real
// hook path: Claude #3a7c (executing, one running Explore subagent, a 300k
// transcript), Claude #9e41 (turn ended with two subagents still running →
// delegating) and Gemini (turn finished → done; Gemini CLI's hooks carry no
// subagent events). Each keeps its own status, listed under the worktree row.

test.beforeEach(async () => {
  await resetMock();
});

test("three agents in one worktree are listed under its row with their own status", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();

  // The worktree row: headline glyph (executing is the most urgent) + count.
  await tui.waitForMatch(/● feature\/pair\b.*⧉ 3 sessions/);
  const lines = await tui.lines();
  const at = lines.findIndex((l) => l.includes("feature/pair"));
  const parent = lines[at];
  expect(parent).not.toContain("◆");
  expect(parent).not.toMatch(/\d+k │/);

  // One row per session, oldest first: connector, own glyph, harness (a short
  // id when two share a harness), status, own subagent badge and context bar.
  const first = lines[at + 1];
  expect(first).toMatch(/├ ● claude #3a7c\s+executing/);
  expect(first).toMatch(/◆ 1 subagent\s/);
  // 300k of 1M: 0.3 × 64 = 19.2 → 19 eighths = two full cells + a 3-eighth
  // part cell drawn as a full-width ▒ (no gap before the ░ track).
  expect(first).toMatch(/██▒░░░░░ 300k │/);
  const second = lines[at + 2];
  expect(second).toMatch(/├ ◑ claude #9e41\s+delegating/);
  expect(second).toContain("◆ 2 subagents");
  const gemini = lines[at + 3];
  expect(gemini).toMatch(/└ ✓ gemini\s+done/);
  expect(gemini).not.toContain("#");
  expect(gemini).not.toContain("◆");
  await tui.screenshot("multi-session");

  // Session rows take no selection index: j from the worktree skips them.
  await tui.selectRow("feature/pair");
  await tui.sendKey("j");
  expect(await tui.selectedName()).toBe("feature/to-delete");

  // Clicking a session row selects its worktree.
  await tui.clickText("gemini  done");
  expect(await tui.selectedName()).toBe("feature/pair");
  await tui.screenshot("multi-session-selected");

  // The help overlay explains the session-count badge.
  await tui.sendKey("?");
  await tui.waitForText("⧉ 2 sessions agents in one worktree, each listed below it with its own status");
  await tui.screenshot("multi-session-help");
  await tui.sendKey("?");
});
