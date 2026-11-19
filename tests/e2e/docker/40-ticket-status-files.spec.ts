import { test, expect } from "@playwright/test";
import { RustTui, resetMock, setupMock } from "./helpers/rust-tui";

// Worktree rows show the Linear ticket's state after its identifier
// (`ENG-123 In Progress`, `In Prog…` below 100 columns; `showLinearStatus`)
// and the PR's changed-file count after the PR cell (`12 files` / `12f`;
// `showPrFiles`). Both are Settings → Dashboard toggles.

test.beforeEach(async () => {
  await resetMock();
});

const STATUS_HELP = "Linear state (In Progress, Done…) beside the ticket";
const STATUS_ROW = /Show ticket status\s+(on|off)/;
const SELECTED_STATUS = /│▌.*\sENG-123 In Prog(ress|…)\s/;
const SELECTED_FILES = /│▌.*#42✓ review\s+12( files|f)\s/;

async function statusValue(tui: RustTui): Promise<string> {
  const m = STATUS_ROW.exec(await tui.getScreenText());
  if (!m) throw new Error(`Setting row not found:\n${await tui.getScreenText()}`);
  return m[1];
}

/** Open Settings and move the cursor onto "Show ticket status". */
async function openOnStatusRow(tui: RustTui): Promise<void> {
  await tui.sendKey("s");
  await tui.waitForText("esc close");
  for (let i = 0; i < 60 && !(await tui.getScreenText()).includes(STATUS_HELP); i++) {
    await tui.sendKey("j");
  }
  await tui.waitForText(STATUS_HELP);
}

/** Toggle "Show ticket status" to `want` and close Settings. */
async function setStatus(tui: RustTui, want: "on" | "off", shot?: string): Promise<void> {
  await openOnStatusRow(tui);
  if ((await statusValue(tui)) !== want) {
    await tui.sendKey(" ");
    await tui.waitForMatch(new RegExp(`Show ticket status\\s+${want}\\b`));
  }
  expect(await tui.getScreenText()).toMatch(/Show PR file count\s+on/);
  if (shot) await tui.screenshot(shot);
  await tui.sendKey("Escape");
  await tui.waitForGone("esc close");
}

test("ticket status and PR file count on the row; the status toggles off in Settings", async ({ page }) => {
  await setupMock({
    gh: {
      number: 42,
      title: "feat: add dashboard feature",
      url: "https://github.com/test/repo/pull/42",
      state: "OPEN",
      isDraft: false,
      reviewDecision: "REVIEW_REQUIRED",
      changedFiles: 12,
      statusCheckRollup: [{ status: "COMPLETED", conclusion: "SUCCESS", name: "build" }],
    },
  });
  const tui = new RustTui(page);
  await tui.open();
  await tui.selectRow("main");
  await tui.waitForMatch(SELECTED_STATUS, 15_000);
  await tui.waitForMatch(SELECTED_FILES, 15_000);
  await tui.screenshot("ticket-status-files");

  try {
    await setStatus(tui, "off", "ticket-status-setting-off");
    // The identifier stays; its state goes. The file count is untouched.
    await tui.waitForMatch(/│▌.*\sENG-123\s/, 5_000);
    await expect.poll(async () => await tui.selectedRow(), { timeout: 5_000 }).not.toMatch(/In Prog/);
    const row = await tui.selectedRow();
    expect(row).toContain("ENG-123");
    expect(row).not.toMatch(/In Prog/);
    expect(row).toMatch(/#42✓ review\s+12( files|f)\s/);
    await tui.screenshot("ticket-status-off");
  } finally {
    await setStatus(tui, "on");
  }
  await tui.waitForMatch(SELECTED_STATUS, 5_000);
});
