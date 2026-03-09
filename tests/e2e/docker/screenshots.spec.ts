import { test } from "@playwright/test";
import { TuiPage } from "./helpers/tui-page.js";
import { setupMock, resetMock } from "./helpers/mock-api-client.js";

test.beforeEach(async () => {
  await resetMock();
});

test("capture dashboard baseline screenshot", async ({ page }) => {
  const tui = new TuiPage(page);
  await tui.goto();
  await tui.waitForText("main", 10_000);

  // Wait for all async data to load
  await page.waitForTimeout(3000);

  await tui.screenshot("baseline-dashboard");
});

test("capture settings screen screenshot", async ({ page }) => {
  const tui = new TuiPage(page);
  await tui.goto();
  await tui.waitForText("main", 10_000);

  await tui.sendKey("s");
  await page.waitForTimeout(1000);

  await tui.screenshot("baseline-settings");
});

test("capture dashboard with PR data screenshot", async ({ page }) => {
  await setupMock({
    gh: {
      number: 42,
      title: "feat: add dashboard feature",
      url: "https://github.com/test/repo/pull/42",
      state: "OPEN",
      isDraft: false,
      reviewDecision: "APPROVED",
      statusCheckRollup: [
        { status: "COMPLETED", conclusion: "SUCCESS", name: "build" },
        { status: "COMPLETED", conclusion: "SUCCESS", name: "test" },
      ],
    },
  });

  const tui = new TuiPage(page);
  await tui.goto();
  await tui.waitForText("main", 10_000);
  await page.waitForTimeout(3000);

  await tui.screenshot("baseline-dashboard-with-pr");
});
