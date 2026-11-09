import { test, expect } from "@playwright/test";
import { RustTui, resetMock } from "./helpers/rust-tui";

test.beforeEach(async () => {
  await resetMock();
});

// Labels are padded to 30 columns, so the value follows 2+ spaces.
const EDITOR_DISPLAY = /Editor display {2,}(\S+)/;
const HELP = "Monitor a built-in editor opens worktrees on";

async function displayValue(tui: RustTui): Promise<string> {
  const m = EDITOR_DISPLAY.exec(await tui.getScreenText());
  if (!m) throw new Error(`Editor display row not found:\n${await tui.getScreenText()}`);
  return m[1];
}

/** Step the selected Editor display row with space until it reads `want`. */
async function cycleTo(tui: RustTui, want: string, max = 6): Promise<void> {
  for (let i = 0; i < max && (await displayValue(tui)) !== want; i++) {
    await tui.sendKey(" ");
    await tui.waitForMatch(EDITOR_DISPLAY);
  }
  expect(await displayValue(tui)).toBe(want);
}

test("Editor display row cycles monitors and is restored", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();

  await tui.sendKey("s");
  await tui.waitForText("esc close");
  await tui.waitForText("Editor display");
  expect(await displayValue(tui)).toBe("auto");
  // The row sits right after Editor command.
  const text = await tui.getScreenText();
  expect(text.indexOf("Editor display")).toBeGreaterThan(text.indexOf("Editor command"));
  expect(text.indexOf("Editor display")).toBeLessThan(text.indexOf("Resume last session on open"));
  await tui.screenshot("editor-display-setting");

  // Mode → Open in → Editor command → Editor display.
  await tui.press("j", 3);
  await tui.waitForText(HELP);

  // auto → 1 → 2.
  await tui.sendKey(" ");
  await tui.waitForMatch(/Editor display {2,}1\b/);
  await tui.sendKey(" ");
  await tui.waitForMatch(/Editor display {2,}2\b/);
  expect(await displayValue(tui)).toBe("2");
  expect(await tui.getScreenText()).toContain("Monitor");
  await tui.screenshot("editor-display-2");
  await tui.sendKey("Escape");
  await tui.waitForGone("esc close");

  // It persisted: a new connection is a new am-tui reading settings.json.
  await page.reload();
  await tui.open();
  await tui.sendKey("s");
  await tui.waitForText("esc close");
  expect(await displayValue(tui)).toBe("2");

  // Restore to auto (2 → 3 → 4 → auto).
  await tui.press("j", 3);
  await tui.waitForText(HELP);
  await cycleTo(tui, "auto");
  await tui.sendKey("Escape");
  await tui.waitForGone("esc close");

  await page.reload();
  await tui.open();
  await tui.sendKey("s");
  await tui.waitForText("esc close");
  expect(await displayValue(tui)).toBe("auto");
  await tui.sendKey("Escape");
});
