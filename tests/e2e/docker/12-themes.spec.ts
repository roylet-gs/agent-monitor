import { test, expect } from "@playwright/test";
import { RustTui, resetMock } from "./helpers/rust-tui";

test.beforeEach(async () => {
  await resetMock();
});

// Help lines of the Theme and Theme builder settings rows.
const THEME = "ansi follows your terminal palette";
const BUILDER = "Make your own theme";

test("built-in themes cycle from the Theme row", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  await tui.openSetting(THEME);
  await tui.waitForMatch(/Theme\s+ansi/);

  await tui.press("ArrowRight", 4);
  await tui.waitForMatch(/Theme\s+catppuccin-latte/);
  await tui.screenshot("theme-catppuccin-latte");

  // Back past ansi to the end of the list: the retro themes.
  await tui.press("ArrowLeft", 5);
  await tui.waitForMatch(/Theme\s+amber-phosphor/);
  await tui.screenshot("theme-amber-phosphor");
  await tui.press("ArrowLeft", 4);
  await tui.waitForMatch(/Theme\s+dos/);
  await tui.sendKey("Escape");
  await tui.waitForGone("esc close");
  await tui.screenshot("theme-dos");

  // Restore.
  await tui.openSetting(THEME);
  await tui.press("ArrowRight", 5);
  await tui.waitForMatch(/Theme\s+ansi/);
  await tui.sendKey("Escape");
});

test("theme builder: pick from the palette, type a hex, save and select", async ({ page }) => {
  const tui = new RustTui(page);
  await tui.open();
  await tui.openSetting(BUILDER);
  await tui.sendKey("Enter");
  await tui.waitForText("Theme builder ");
  await tui.waitForText("Name ansi-custom");
  await tui.waitForText("Preview");
  await tui.screenshot("theme-builder");

  // accent: cyan → two rows down the palette = the first curated row's violet.
  await tui.press("j", 2);
  await tui.sendKey("Enter");
  await tui.waitForText("accent — pick a colour");
  await tui.press("ArrowDown", 2);
  await tui.screenshot("theme-builder-palette");
  await tui.sendKey("Enter");
  await tui.waitForMatch(/accent\s+██ #c4b5fd/);

  // border: typed hex.
  await tui.sendKey("j");
  await tui.sendKey("#");
  await tui.type("123456");
  await tui.sendKey("Enter");
  await tui.waitForMatch(/border\s+██ #123456/);
  await tui.waitForText("• unsaved");

  // Rename and save: the builder closes and the theme is selected.
  await tui.sendKey("n");
  await tui.press("Backspace", 11);
  await tui.type("e2e-theme");
  await tui.sendKey("Enter");
  await tui.waitForText("Name e2e-theme");
  await tui.sendKey("w");
  await tui.waitForGone("Theme builder ");
  await tui.waitForMatch(/Theme\s+e2e-theme/);
  await tui.sendKey("Escape");

  // Persisted: a new process starts on it, and reopening the builder edits it.
  await page.reload();
  await tui.open();
  await tui.openSetting(BUILDER);
  expect(await tui.getScreenText()).toMatch(/Theme\s+e2e-theme/);
  await tui.sendKey("Enter");
  await tui.waitForText("Name e2e-theme");
  await tui.waitForMatch(/accent\s+██ #c4b5fd/);
  await tui.sendKey("Escape");
  await tui.waitForGone("Theme builder ");

  // Restore: custom themes come last, so the next one wraps to ansi.
  await tui.sendKey("k");
  await tui.sendKey("ArrowRight");
  await tui.waitForMatch(/Theme\s+ansi/);
  await tui.sendKey("Escape");
});
