import { test, expect } from "@playwright/test";
import { RustTui, resetMock } from "./helpers/rust-tui";

// `e` in Settings opens settings.json in the configured `ide` (default
// cursor). The container has no `cursor` CLI (and no macOS `open`), so the
// launch fails: the TUI must stay up and show the error popup rather than
// handing the terminal to $EDITOR. The live reload of an externally edited
// settings.json is exercised with two connections (two am-tui processes on
// the same data dir): a change saved by one appears in the other.

test.beforeEach(async () => {
  await resetMock();
});

const NOTICE = "⬆ v9.9.9 available";
const CHECK = /Check for updates\s+(on|off)/;
const CHECK_HELP = "Show a notice when a newer am release is on GitHub";

async function checkValue(tui: RustTui): Promise<string> {
  const m = CHECK.exec(await tui.getScreenText());
  if (!m) throw new Error(`Setting row not found:\n${await tui.getScreenText()}`);
  return m[1];
}

test("e with the default ide reports the failed launch and keeps the TUI", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();

  await tui.sendKey("s");
  await tui.waitForText("esc close");
  await tui.sendKey("e");

  // No IDE in the container: the launch fails on a background thread and the
  // error shows as a popup; the dashboard and settings are still on screen.
  await tui.waitForText("✗ Couldn't open Cursor");
  const screen = await tui.getScreenText();
  expect(screen).toContain("─Worktrees");
  expect(screen).not.toContain("~\n~"); // not vi's empty-buffer tildes
  await tui.screenshot("edit-settings-file-error");

  // Any key closes the popup; the settings editor is still open underneath.
  await tui.sendKey("Escape");
  await tui.waitForGone("Couldn't open Cursor");
  await tui.waitForText("esc close");
  await tui.screenshot("edit-settings-file-after");
  await tui.sendKey("Escape");
  await tui.waitForGone("esc close");
  expect(await tui.getScreenText()).toContain("─Worktrees");
});

test("settings.json changed by another process is reloaded live", async ({ page }) => {
  const watcher = new RustTui(page);
  await watcher.open();
  await watcher.waitForText(NOTICE);

  // A second connection = a second am-tui process writing settings.json.
  const editorPage = await page.context().newPage();
  const editor = new RustTui(editorPage);
  try {
    await editor.open();
    await editor.openSetting(CHECK_HELP);
    expect(await checkValue(editor)).toBe("on");
    await editor.sendKey(" ");
    expect(await checkValue(editor)).toBe("off");

    // The first process notices the new mtime within ~1 s and hides the notice.
    await watcher.waitForGone(NOTICE);
    expect((await watcher.lines())[0]).toContain("─Worktrees");
    await watcher.screenshot("edit-settings-file-live-reload");

    // Restore from the second process; the first picks that up too.
    await editor.sendKey(" ");
    expect(await checkValue(editor)).toBe("on");
    await editor.sendKey("Escape");
    await editor.waitForGone("esc close");
    await watcher.waitForText(NOTICE);
  } finally {
    await editorPage.close();
  }
});
