import { test, expect } from "@playwright/test";
import { RustTui, FRESH_URL, resetMock } from "./helpers/rust-tui";

// FRESH_URL (port 7682) starts am-tui with an empty AM_DATA_DIR and HOME on
// every connection, so settings.json has no setupCompleted → first-run wizard.

test.beforeEach(async () => {
  await resetMock();
});

test("seeded dashboard skips the wizard", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  expect(await tui.getScreenText()).not.toContain("Welcome to agent-monitor");
});

test("first run walks the setup wizard to the dashboard", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.openUrl(FRESH_URL);
  await tui.waitForText("Welcome to agent-monitor", 10_000);

  // 1. Editor
  await tui.waitForText("Where should Enter open a worktree?");
  await tui.waitForText("● Cursor");
  await tui.sendKey("j");
  await tui.waitForText("● VS Code");
  await tui.screenshot("rust-setup-editor");
  await tui.sendKey("Enter");

  // 2. Claude hooks: install into the (temporary) ~/.claude/settings.json.
  await tui.waitForText("Hooks are not installed yet");
  await tui.waitForMatch(/Native hook binary: \S*am-hook/);
  await tui.sendKey("y");

  // 3. Repositories: add /work.
  await tui.waitForText("Add the git repositories whose worktrees you want to watch.");
  await tui.sendKey("Backspace");
  await tui.sendKey("Backspace");
  await tui.type("/work");
  await tui.sendKey("Enter");
  await tui.waitForText("✓ work", 15_000);
  await tui.screenshot("rust-setup-repos");
  await tui.sendKey("Enter"); // empty input → next

  // 4. Integrations
  await tui.waitForText("GitHub PR & CI status (via gh)");
  await tui.sendKey("Enter");

  await tui.waitForGone("Welcome to agent-monitor");
  await tui.waitForText("feature/to-delete", 10_000);
  // A fresh settings.json has hideMainBranch on (the default), and no Linear key.
  const text = await tui.getScreenText();
  expect(text).not.toMatch(/\S main\s/);
  expect(text).not.toContain("ENG-123");
  await tui.screenshot("rust-setup-done");
});
