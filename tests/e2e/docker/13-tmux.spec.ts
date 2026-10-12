import { test, expect } from "@playwright/test";
import { RustTui, resetMock } from "./helpers/rust-tui";

// Port 7683: a fresh tmux server per connection with agent-monitor.tmux loaded
// (as TPM would) and `status-right '#{agent_monitor}'`, on the seeded data dir.
// main is delegating (hooks) and has a `claude --bg` session from the fake
// `claude agents --json` (e2e/bin/claude); the standalone session is executing.
const TMUX_URL = process.env.TMUX_URL || "http://localhost:7683";

test.beforeEach(async () => {
  await resetMock();
});

async function openTmux(tui: RustTui): Promise<void> {
  await tui.openUrl(TMUX_URL);
  // The status bar segment from `am summary --format tmux`.
  await tui.waitForMatch(/◑1/, 15_000);
}

async function prefix(tui: RustTui, key: string): Promise<void> {
  await tui.sendKey("Control+b");
  await tui.sendKey(key);
}

test("status bar shows agent counts from am summary", async ({ page }) => {
  const tui = new RustTui(page);
  await openTmux(tui);
  const status = await tui.keyBar();
  expect(status).toMatch(/◑1/); // main: delegating
  expect(status).toMatch(/●1/); // standalone session: executing
  await tui.screenshot("tmux-status");
});

test("prefix A opens the dashboard popup; c attaches to the background session in a new window", async ({ page }) => {
  const tui = new RustTui(page);
  await openTmux(tui);
  await prefix(tui, "Shift+A");
  await tui.waitForText("─Worktrees", 15_000);
  await tui.waitForMatch(/◑ main\b.* bg\b/, 15_000);
  await tui.selectRow("main");
  await tui.screenshot("tmux-popup");

  await tui.sendKey("c");
  // The popup closes and a new tmux window runs `claude attach bg-e2e`.
  await tui.waitForText("fake-claude attach bg-e2e", 15_000);
  await tui.waitForGone("─Worktrees", 10_000);
  await tui.screenshot("tmux-attach-window");
});

test("prefix S toggles the sidebar", async ({ page }) => {
  const tui = new RustTui(page);
  await openTmux(tui);
  await prefix(tui, "Shift+S");
  await tui.waitForText("─Worktrees", 15_000);
  // Compact rows: no ticket/git/age columns, short key bar with `? help`.
  await tui.waitForText("? help", 10_000);
  await tui.screenshot("tmux-sidebar");
  await prefix(tui, "Shift+S");
  await tui.waitForGone("─Worktrees", 10_000);
});
