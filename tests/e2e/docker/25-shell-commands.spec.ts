import { test, expect } from "@playwright/test";
import { RustTui, resetMock } from "./helpers/rust-tui";

// e2e/entrypoint.sh leaves a Claude-style Bash tool shell
// (`bash -c "source ~/.claude/shell-snapshots/… && sleep infinity"`) running
// in feature/to-delete. procscan counts it (every 2 s), and since it matches
// the seeded runningProcessFilter ("sleep infinity") the row is highlighted:
// a bold teal `$ 1 command` badge and a teal name.

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
  await tui.sendKey("h");
  await tui.waitForText("$ 1 command an agent's shell command");
  await tui.screenshot("shell-command-help");
  await tui.sendKey("h");
});

const FILTER_HELP = "processes and agent commands whose command line";

test("a shell command that doesn't match the process filter is not highlighted", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  await tui.waitForMatch(/feature\/to-delete \$ 1 command\b/);

  // "sleep infinityx" no longer matches: the badge goes.
  await tui.openSetting(FILTER_HELP);
  await tui.sendKey("Enter");
  await tui.type("x");
  await tui.sendKey("Enter");
  await tui.waitForText("sleep infinityx");
  await tui.sendKey("Escape");
  await tui.waitForGone("esc close");
  await tui.waitForGone("$ 1 command");
  expect((await tui.lines()).some((l) => /\$ ?\d+( commands?)?\b/.test(l) && l.includes("feature/to-delete"))).toBe(false);
  await tui.screenshot("shell-command-filtered-out");

  // Restore the seeded filter; the highlight comes back.
  await tui.openSetting(FILTER_HELP);
  await tui.sendKey("Enter");
  await tui.sendKey("Backspace");
  await tui.sendKey("Enter");
  await tui.sendKey("Escape");
  await tui.waitForGone("esc close");
  await tui.waitForMatch(/feature\/to-delete \$ 1 command\b/);
});
