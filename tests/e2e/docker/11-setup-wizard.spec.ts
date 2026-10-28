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

  // 0. Mode: the full dashboard is preselected.
  await tui.waitForText("What do you want am for?");
  await tui.waitForText("● Agents & worktrees");
  await tui.screenshot("setup-mode");
  await tui.sendKey("Enter");

  // 1. Editor
  await tui.waitForText("Where should Enter open a worktree?");
  await tui.waitForText("● VS Code");
  await tui.sendKey("j");
  await tui.waitForText("● Cursor");
  // ↑ from the top wraps to the custom row, where letters are the command.
  await tui.sendKey("k");
  await tui.sendKey("ArrowUp");
  await tui.waitForText("● Custom command:");
  await tui.sendKey("Enter");
  await tui.waitForText("Type the command that opens a folder");
  await tui.type("subl -n");
  await tui.waitForText("● Custom command: subl -n");
  await tui.waitForText("in place of {path}");
  await tui.screenshot("setup-editor");
  await tui.sendKey("Enter");

  // 2. Agent hooks: install into the (temporary) ~/.claude/settings.json.
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
  await tui.screenshot("setup-repos");
  await tui.sendKey("Enter"); // empty input → next

  // 4. Integrations
  await tui.waitForText("GitHub PR & CI status (via gh)");
  await tui.sendKey("Enter");

  // 5. Review requests (GitHub is on): defaults are me + my teams, no filters.
  await tui.waitForText("Which pull requests are waiting on your review?");
  await tui.waitForText("[x] Show PRs waiting for my review");
  await tui.waitForText("● Me and my teams");
  await tui.screenshot("setup-reviews");
  await tui.sendKey("Enter");

  // 6. Context window: 1M is preselected; keep it.
  await tui.waitForText("How many tokens of context should the bar measure against?");
  await tui.waitForText("● 1M (default)");
  await tui.screenshot("setup-context");
  await tui.sendKey("Enter");

  await tui.waitForGone("Welcome to agent-monitor");
  // First sync + git scan of the new repo can be slow on a cold container.
  await tui.waitForText("feature/to-delete", 20_000);
  // A fresh settings.json has hideMainBranch on (the default), and no Linear key.
  const text = await tui.getScreenText();
  expect(text).not.toMatch(/\S main\s/);
  expect(text).not.toContain("ENG-123");
  await tui.screenshot("setup-done");
});
