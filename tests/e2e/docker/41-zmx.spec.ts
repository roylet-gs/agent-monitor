import { test, expect } from "@playwright/test";
import { RustTui, resetMock } from "./helpers/rust-tui";

test.beforeEach(async () => {
  await resetMock();
});

// Labels are padded to 30 columns, so the value follows 2+ spaces.
const OPEN_IN = /Open in {2,}(\S+)/;

async function openIn(tui: RustTui): Promise<string> {
  const m = OPEN_IN.exec(await tui.getScreenText());
  if (!m) throw new Error(`Open in row not found:\n${await tui.getScreenText()}`);
  return m[1];
}

/** In Settings, on the Open in row: step it with space until it reads `want`. */
async function cycleTo(tui: RustTui, want: string, max = 10): Promise<void> {
  for (let i = 0; i < max && (await openIn(tui)) !== want; i++) {
    await tui.sendKey(" ");
    await tui.waitForMatch(OPEN_IN);
  }
  expect(await openIn(tui)).toBe(want);
}

async function setOpenIn(tui: RustTui, want: string): Promise<void> {
  await tui.sendKey("s");
  await tui.waitForText("esc close");
  await tui.sendKey("j"); // past "Mode"
  await tui.waitForText("Where Enter opens worktrees");
  await cycleTo(tui, want);
  await tui.sendKey("Escape");
  await tui.waitForGone("esc close");
}

// The container has no zmx binary, so this covers everything up to running
// it: am's session name, the peek pane, and what Enter says when zmx is missing.
test("Open in zmx: peek pane names am's session, Enter says zmx is missing", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();

  try {
    await setOpenIn(tui, "zmx");
    await tui.selectRow("main");

    // P opens the peek pane on main's am-named session; zmx history fails here.
    await tui.sendKey("P");
    await tui.waitForText("Peek zmx am.work.main");
    await tui.waitForText("zmx isn't installed");
    await tui.screenshot("zmx-peek-pane");

    // P again closes it.
    await tui.sendKey("P");
    await tui.waitForGone("Peek zmx");

    // Enter would attach to the session; without zmx it explains instead.
    await tui.sendKey("Enter");
    await tui.waitForText("zmx isn't installed. Install it");
    await tui.screenshot("zmx-not-installed");
    await tui.sendKey("Escape");
    await tui.waitForGone("zmx isn't installed. Install it");
  } finally {
    await setOpenIn(tui, "vscode");
  }

  // Back on vscode no session is derived, so the pane has nothing to show.
  await tui.selectRow("main");
  await tui.sendKey("P");
  const text = await tui.getScreenText();
  expect(text).not.toContain("Peek zmx am.work.main");
});
