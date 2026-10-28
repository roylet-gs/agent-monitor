import { test, expect } from "@playwright/test";
import { RustTui, resetMock } from "./helpers/rust-tui";

// Seeded in e2e/entrypoint.sh by replaying native Gemini CLI hook events
// (SessionStart → BeforeAgent → Notification/ToolPermission) through
// `am hook-event --agent gemini` on the untracked /home/user/gemini-project.

test.beforeEach(async () => {
  await resetMock();
});

test("a Gemini CLI session shows its status and a ✦ gemini badge", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  await tui.waitForMatch(/│Other sessions ─+/);
  // ToolPermission notification → waiting, like a Claude permission prompt.
  await tui.waitForMatch(/\/home\/user\/gemini-project ✦ gemini/);

  await tui.selectRow("/home/user/gemini-project");
  expect(await tui.selectedRow()).toMatch(/\/home\/user\/gemini-project ✦ gemini/);
  // Claude sessions stay unbadged.
  const claudeRow = (await tui.lines()).find((l) => l.includes("/home/user/my-project")) ?? "";
  expect(claudeRow).not.toContain("✦");
  // Context usage comes from the session's own harness plugin. Gemini has no
  // transcript reader, so its row has no context bar or token count, even
  // with a Claude-shaped transcript planted under its session id
  // (e2e/entrypoint.sh). main's bar shows the column is on.
  await tui.waitForMatch(/ main\b.*150k │/);
  // (Drop the selection bar: the row is selected and `▌` is also a bar glyph.)
  const geminiRow = ((await tui.lines()).find((l) => l.includes("/home/user/gemini-project")) ?? "").replace("│▌", "│ ");
  expect(geminiRow).not.toMatch(/[░█▏▎▍▌▋▊▉]/);
  expect(geminiRow).not.toMatch(/\d+(\.\d+)?[kM] │/);
  await tui.screenshot("gemini-session");

  // The help overlay explains the badge.
  await tui.sendKey("h");
  await tui.waitForText("session from another harness plugin, by id (no badge: Claude Code)");
  await tui.screenshot("gemini-help");
  await tui.sendKey("h");
});

test("a Codex CLI session is found from its payload cwd and shows ✦ codex", async ({ page }) => {
  // Seeded with `am hook-event --agent codex --worktree ""`, ending on a
  // PermissionRequest → waiting (◐).
  const tui = new RustTui(page);
  await tui.open();
  await tui.waitForMatch(/│Other sessions ─+/);
  await tui.waitForMatch(/◐ \/home\/user\/codex-project ✦ codex/);
  await tui.selectRow("/home/user/codex-project");
  await tui.screenshot("codex-session");
});

test("Settings → Harness plugins lists the plugins and toggles one", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  await tui.openSetting("Turn harnesses on/off, install their hooks, pick the default");
  await tui.waitForMatch(/Harness plugins…\s+8 of 8 on · default claude/);
  await tui.sendKey("Enter");
  await tui.waitForText("Harness plugins — space on/off · i install hooks · d default · esc back");
  await tui.waitForMatch(/\[x\] Claude Code\s+built-in/);
  await tui.waitForMatch(/\[x\] Gemini CLI\s+built-in/);
  // Every built-in harness is listed.
  for (const name of ["Codex CLI", "Qwen Code", "Factory Droid", "GitHub Copilot CLI", "Cursor", "CodeBuddy Code"]) {
    await tui.waitForMatch(new RegExp(`\\[x\\] ${name}\\s+built-in`));
  }
  await tui.screenshot("harness-plugins");

  // Switch Gemini off and back on; the summary row follows.
  await tui.sendKey("j");
  await tui.sendKey(" ");
  await tui.waitForMatch(/\[ \] Gemini CLI/);
  await tui.waitForText("Gemini CLI off");
  await tui.screenshot("harness-plugins-gemini-off");
  await tui.sendKey(" ");
  await tui.waitForMatch(/\[x\] Gemini CLI/);
  await tui.sendKey("Escape");
  await tui.waitForMatch(/Harness plugins…\s+8 of 8 on · default claude/);
  await tui.sendKey("Escape");
  await tui.waitForGone("esc close");
});
