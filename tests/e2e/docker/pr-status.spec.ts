import { test, expect } from "@playwright/test";
import { TuiPage } from "./helpers/tui-page.js";
import { setupMock, resetMock } from "./helpers/mock-api-client.js";

test.beforeEach(async () => {
  await resetMock();
});

test("shows PR status when PR exists", async ({ page }) => {
  await setupMock({
    gh: {
      number: 42,
      title: "feat: add dashboard feature",
      url: "https://github.com/test/repo/pull/42",
      state: "OPEN",
      isDraft: false,
      reviewDecision: "REVIEW_REQUIRED",
      statusCheckRollup: [
        { status: "COMPLETED", conclusion: "SUCCESS", name: "build" },
      ],
    },
  });

  const tui = new TuiPage(page);
  await tui.goto();
  await tui.waitForText("main", 10_000);

  // Wait for PR info to be fetched and rendered
  await page.waitForTimeout(3000);

  await tui.screenshot("pr-status-open");
});

test("shows draft PR status", async ({ page }) => {
  await setupMock({
    gh: {
      number: 43,
      title: "wip: draft feature",
      url: "https://github.com/test/repo/pull/43",
      state: "OPEN",
      isDraft: true,
      reviewDecision: "",
      statusCheckRollup: [],
    },
  });

  const tui = new TuiPage(page);
  await tui.goto();
  await tui.waitForText("main", 10_000);

  await page.waitForTimeout(3000);

  await tui.screenshot("pr-status-draft");
});

test("handles no PR gracefully", async ({ page }) => {
  await setupMock({ gh: null });

  const tui = new TuiPage(page);
  await tui.goto();
  await tui.waitForText("main", 10_000);

  await page.waitForTimeout(3000);

  // Dashboard should still render without PR info
  const text = await tui.getScreenText();
  expect(text).toContain("main");

  await tui.screenshot("pr-status-none");
});
