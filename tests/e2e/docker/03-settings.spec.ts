import { test, expect } from "@playwright/test";
import { RustTui, resetMock } from "./helpers/rust-tui";

test.beforeEach(async () => {
  await resetMock();
});

const RESUME = /Resume last session on open\s+(on|off)/;

async function resumeValue(tui: RustTui): Promise<string> {
  const m = RESUME.exec(await tui.getScreenText());
  if (!m) throw new Error(`Setting row not found:\n${await tui.getScreenText()}`);
  return m[1];
}

test("settings editor opens, a toggle persists across restarts", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();

  await tui.sendKey("s");
  await tui.waitForText("Settings ");
  await tui.waitForText("General");
  await tui.waitForText("esc close");
  await tui.screenshot("settings");

  const before = await resumeValue(tui);
  const after = before === "on" ? "off" : "on";
  await tui.sendKey("j"); // cursor (on "Mode") → "Open in"
  await tui.waitForText("Which editor/IDE Enter opens worktrees in");
  await tui.sendKey("j"); // → "Editor command"
  await tui.waitForText("For Open in = custom");
  await tui.sendKey("j"); // → "Resume last session on open"
  await tui.waitForText("Enter also resumes the worktree's most recent agent session");
  await tui.sendKey(" ");
  expect(await resumeValue(tui)).toBe(after);
  await tui.sendKey("Escape");
  await tui.waitForGone("esc close");

  // A new connection is a new am-tui process: the value came from settings.json.
  await page.reload();
  await tui.open();
  await tui.sendKey("s");
  await tui.waitForText("esc close");
  expect(await resumeValue(tui)).toBe(after);

  // Restore (Mode → Open in → Editor command → Resume last session).
  await tui.press("j", 3);
  await tui.sendKey(" ");
  expect(await resumeValue(tui)).toBe(before);
  await tui.sendKey("Escape");
});

// Labels are padded to 30 columns, so the value follows 2+ spaces (the help
// line "For Open in = custom" has only one).
const OPEN_IN = /Open in {2,}(\S+)/;
const EDITOR_CMD = /Editor command {2,}(\S+(?: \S+)*)/;

async function rowValue(tui: RustTui, re: RegExp): Promise<string> {
  const m = re.exec(await tui.getScreenText());
  if (!m) throw new Error(`Setting row ${re} not found:\n${await tui.getScreenText()}`);
  return m[1];
}

/** Step the selected Choice row with space until it reads `want`. */
async function cycleTo(tui: RustTui, re: RegExp, want: string, max = 10): Promise<void> {
  for (let i = 0; i < max && (await rowValue(tui, re)) !== want; i++) {
    await tui.sendKey(" ");
  }
  expect(await rowValue(tui, re)).toBe(want);
}

test("Open in cycles to custom and takes an editor command", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();

  await tui.sendKey("s");
  await tui.waitForText("esc close");
  await tui.sendKey("j"); // past "Mode"
  await tui.waitForText("Which editor/IDE Enter opens worktrees in");
  expect(await rowValue(tui, OPEN_IN)).toBe("vscode");
  expect(await rowValue(tui, EDITOR_CMD)).toBe("—");

  // Every choice in order: vscode → cursor → zed → windsurf → terminal → custom.
  const seen: string[] = [];
  for (const want of ["cursor", "zed", "windsurf", "terminal", "custom"]) {
    await tui.sendKey(" ");
    await tui.waitForMatch(new RegExp(`Open in {2,}${want}\\b`));
    seen.push(await rowValue(tui, OPEN_IN));
  }
  expect(seen).toEqual(["cursor", "zed", "windsurf", "terminal", "custom"]);

  // Editor command: Enter edits (prefilled with the current, empty value).
  await tui.sendKey("j");
  await tui.waitForText("For Open in = custom");
  await tui.sendKey("Enter");
  await tui.type("subl -n");
  await tui.waitForText("subl -n▏");
  await tui.sendKey("Enter");
  await tui.waitForGone("subl -n▏");
  expect(await rowValue(tui, EDITOR_CMD)).toBe("subl -n");
  expect(await rowValue(tui, OPEN_IN)).toBe("custom");
  await tui.screenshot("settings-open-in-custom");

  // Restore: the edit buffer starts with "subl -n", so clear it.
  await tui.sendKey("Enter");
  await tui.waitForText("subl -n▏");
  await tui.press("Backspace", 7);
  await tui.waitForGone("subl -n▏");
  await tui.sendKey("Enter");
  await tui.waitForMatch(/Editor command {2,}—/);
  expect(await rowValue(tui, EDITOR_CMD)).toBe("—");

  await tui.sendKey("k");
  await tui.waitForText("Which editor/IDE Enter opens worktrees in");
  await cycleTo(tui, OPEN_IN, "vscode");
  await tui.sendKey("Escape");
  await tui.waitForGone("esc close");

  // Restored values survive a restart.
  await page.reload();
  await tui.open();
  await tui.sendKey("s");
  await tui.waitForText("esc close");
  expect(await rowValue(tui, OPEN_IN)).toBe("vscode");
  expect(await rowValue(tui, EDITOR_CMD)).toBe("—");
  await tui.sendKey("Escape");
});
