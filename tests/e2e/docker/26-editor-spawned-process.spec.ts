import { test, expect } from "@playwright/test";
import { RustTui, resetMock } from "./helpers/rust-tui";

// e2e/entrypoint.sh starts a fake editor (a copy of `timeout` under
// ~/.vscode-server, cwd /) that forks + execs `e2e-dev-server 86400` (a copied
// `sleep`) in feature/pair, with no shell in between, like VS Code's npm
// scripts panel running `pnpm start:dev`. Without a filter such a process is
// hidden as editor tooling (language servers, extension helpers…); when the
// runningProcessFilter names it, it is a process the user asked to see, so
// feature/pair shows the ▶ running-processes badge.

const DETECT_HELP = "Show processes running inside each worktree";
const FILTER_HELP = "Only show processes and agent commands whose command line contains this";
const SEEDED_FILTER = "sleep infinity";

test.beforeEach(async () => {
  await resetMock();
});

/** The worktree row for `branch` (not its session rows). */
function rowOf(lines: string[], branch: string): string {
  return lines.find((l) => new RegExp(`\\s${branch.replace("/", "\\/")}\\b`).test(l)) ?? "";
}

/**
 * Open Settings, set "Detect dev servers" to `detect` and the process filter
 * to `filter`, then close Settings. Idempotent, so a retry starts clean.
 */
async function configure(tui: RustTui, detect: boolean, filter: string): Promise<void> {
  await tui.openSetting(DETECT_HELP);
  const detectRow = (await tui.lines()).find((l) => l.includes("Detect dev servers")) ?? "";
  if (/\bon\b/.test(detectRow) !== detect) {
    await tui.sendKey("Enter");
    await tui.waitForMatch(new RegExp(`Detect dev servers\\s+${detect ? "on" : "off"}\\b`));
  }
  await tui.sendKey("j");
  await tui.waitForText(FILTER_HELP);
  await tui.sendKey("Enter");
  // Every filter this spec sets is shorter than 20 characters. A Backspace on
  // an empty buffer doesn't redraw, so each extra one waits out sendKey's 300 ms.
  await tui.press("Backspace", 20);
  if (filter) await tui.type(filter);
  await tui.sendKey("Enter");
  if (filter) await tui.waitForMatch(new RegExp(`Process filter\\s+${filter}`));
  await tui.sendKey("Escape");
  await tui.waitForGone("esc close");
}

test("a process an editor started directly shows when the filter names it", async ({ page }) => {
  // Four trips through Settings (incl. the restore) take longer than the default 45 s.
  test.setTimeout(120_000);
  const tui = new RustTui(page);
  await tui.open();

  try {
    await configure(tui, true, "e2e-dev-server");
    await tui.waitForMatch(/\sfeature\/pair\b[^│]*▶/);
    const lines = await tui.lines();
    // Only feature/pair runs it; to-delete's `sleep infinity` doesn't match.
    expect(rowOf(lines, "feature/to-delete")).not.toContain("▶");
    expect(lines.filter((l) => l.includes("▶"))).toHaveLength(1);
    await tui.selectRow("feature/pair");
    await tui.screenshot("editor-spawned-process");

    // A filter that doesn't name it: the badge goes.
    await configure(tui, true, "e2e-other");
    await tui.waitForGone("▶");
    expect(rowOf(await tui.lines(), "feature/pair")).not.toContain("▶");

    // No filter: editor-spawned tooling stays hidden, but to-delete's
    // `sleep infinity` (under an agent's shell) is listed.
    await configure(tui, true, "");
    await tui.waitForMatch(/\sfeature\/to-delete\b[^│]*▶/);
    expect(rowOf(await tui.lines(), "feature/pair")).not.toContain("▶");
    await tui.screenshot("editor-spawned-process-no-filter");
  } finally {
    // Restore the seeded settings.
    await configure(tui, false, SEEDED_FILTER);
  }
  await tui.waitForGone("▶");
  await tui.waitForMatch(/feature\/to-delete \$ 1 command\b/);
});
