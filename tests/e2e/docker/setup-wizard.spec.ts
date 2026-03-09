import { test, expect } from "@playwright/test";
import { TuiPage } from "./helpers/tui-page.js";

// Note: This test would require a separate app container without setupCompleted.
// For now, it verifies the seeded app skips the wizard correctly.

test("seeded app skips setup wizard and shows dashboard", async ({ page }) => {
  const tui = new TuiPage(page);
  await tui.goto();

  // If setup wizard was shown, we'd see prompts like "Welcome" or "API key"
  // Instead, we should see the dashboard with the main branch
  await tui.waitForText("main", 10_000);

  const text = await tui.getScreenText();
  expect(text).toContain("main");

  await tui.screenshot("setup-wizard-skipped");
});
