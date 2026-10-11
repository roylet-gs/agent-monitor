import { test, expect } from "@playwright/test";
import { RustTui, resetMock } from "./helpers/rust-tui";

// e2e/entrypoint-rust.sh seeds main through `am-rs hook-event`:
// UserPromptSubmit → SubagentStart (Explore, agent a1b2c3d4, description from
// its meta.json) → PreToolUse Grep → Stop while the subagent still runs, which
// leaves main "delegating".

test.beforeEach(async () => {
  await resetMock();
});

test("delegating worktree shows its running subagent", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();

  // Row badge ◆1 and the status-bar count for delegating.
  await tui.waitForMatch(/◆ main\s.*◆1/);
  expect(await tui.statusBar()).toMatch(/◆ 1/);

  await tui.selectRow("main");
  expect(await tui.contextLine()).toContain("session 3f2a91c8-7b4d-4e0a-9c1f-8d2e5a6b7c90");
  await tui.waitForText("[2] Subagents · main");
  await tui.waitForText("Survey auth module");
  const text = await tui.getScreenText();
  expect(text).toContain("Explore");
  expect(text).toMatch(/└ Grep\s+verifyToken/);
  expect(text).toContain("1 running · 0 done");
  await tui.screenshot("rust-subagents");
});

test("a toggles the Subagents pane between this worktree and all", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  await tui.selectRow("feature/to-delete");
  await tui.waitForText("No subagents running");

  await tui.sendKey("a");
  await tui.waitForText("[2] Subagents · all worktrees");
  await tui.waitForText("Survey auth module");
  await tui.sendKey("2");
  expect(await tui.statusBar()).toContain(" SUBAGENTS ");
  await tui.screenshot("rust-subagents-all");
  await tui.sendKey("a");
  await tui.waitForText("No subagents running");
});
