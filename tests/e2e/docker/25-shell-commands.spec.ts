import { test, expect } from "@playwright/test";
import { RustTui, resetMock } from "./helpers/rust-tui";

// e2e/entrypoint.sh leaves a Claude-style Bash tool shell
// (`bash -c "source ~/.claude/shell-snapshots/… && sleep infinity"`) running
// in feature/to-delete. procscan counts it (every 2 s), and the row is
// highlighted: a bold teal `$ 1 command` badge and a teal name.

test.beforeEach(async () => {
  await resetMock();
});

test("a worktree running an agent's shell command is highlighted", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();

  await tui.waitForMatch(/feature\/to-delete \$ 1 command\b/);
  const lines = await tui.lines();
  // Only that worktree carries the badge.
  const badged = lines.filter((l) => /\$ \d+ commands?\b/.test(l));
  expect(badged).toHaveLength(1);
  expect(badged[0]).toContain("feature/to-delete");
  expect(lines.find((l) => /\bmain\b/.test(l) && l.includes("◑"))).not.toContain("$ 1 command");
  await tui.screenshot("shell-command-badge");

  await tui.selectRow("feature/to-delete");
  expect(await tui.selectedRow()).toMatch(/feature\/to-delete \$ 1 command/);
  await tui.screenshot("shell-command-badge-selected");

  // The help overlay explains the badge.
  await tui.sendKey("?");
  await tui.waitForText("$ 1 command an agent's shell command");
  await tui.screenshot("shell-command-help");
  await tui.sendKey("?");
});
