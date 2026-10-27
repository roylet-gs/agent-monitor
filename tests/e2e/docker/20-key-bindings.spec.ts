import { test, expect } from "@playwright/test";
import { RustTui, resetMock } from "./helpers/rust-tui";

test.beforeEach(async () => {
  await resetMock();
});

const FORM = "tab next  ⏎ create  esc cancel";

async function openKeyBindings(tui: RustTui): Promise<void> {
  await tui.sendKey("s");
  await tui.waitForText("esc close");
  await tui.clickText("Key bindings…");
  await tui.waitForText("Choose the dashboard's keys");
  await tui.sendKey("Enter");
  await tui.waitForText("Key bindings — ⏎ change");
}

test("a dashboard key can be rebound in Settings → Key bindings, then reset", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  expect(await tui.keyBar()).toContain("n new");

  await openKeyBindings(tui);
  // Help is on h; View logs is no longer a key (it's in Settings).
  await tui.waitForMatch(/Help\s+h\s/);
  // Each key also says what it does on the review page.
  await tui.waitForText("On the review page");
  await tui.waitForMatch(/Open worktree\s+⏎\s+pull, or open the pulled one/);
  expect(await tui.getScreenText()).not.toMatch(/View logs\s+L/);
  await tui.screenshot("key-bindings-help-h");
  await tui.press("j", 4); // Open worktree → … → New worktree
  await tui.waitForText("New worktree form");
  await tui.sendKey("Enter");
  await tui.waitForText("press a key…");
  await tui.sendKey("Shift+N");
  await tui.waitForMatch(/New worktree\s+N\s+\(default n\)/);
  await tui.screenshot("key-bindings");

  // A fixed key is refused.
  await tui.press("j");
  await tui.sendKey("Enter");
  await tui.sendKey("ArrowUp");
  await tui.waitForText("is fixed");

  await tui.sendKey("Escape"); // back to the field list
  await tui.waitForText("1 changed");
  await tui.sendKey("Escape");
  await tui.waitForGone("esc close");

  // The key bar follows, N opens the form and n no longer does.
  expect(await tui.keyBar()).toContain("N new");
  await tui.sendKey("n");
  expect(await tui.getScreenText()).not.toContain(FORM);
  await tui.sendKey("Shift+N");
  await tui.waitForText(FORM);
  await tui.screenshot("key-bindings-rebound");
  await tui.sendKey("Escape");
  await tui.waitForGone(FORM);

  // It survives a restart (it lives in settings.json); then reset it with x.
  await page.reload();
  await tui.open();
  expect(await tui.keyBar()).toContain("N new");
  await openKeyBindings(tui);
  await tui.press("j", 4);
  await tui.sendKey("x");
  // No "(default n)" any more; the review-page column says n does nothing there.
  await tui.waitForMatch(/New worktree {2,}n {2,}— {2,}│/);
  await tui.sendKey("Escape");
  await tui.waitForText("defaults");
  await tui.sendKey("Escape");
  await tui.waitForGone("esc close");
  expect(await tui.keyBar()).toContain("n new");
});
