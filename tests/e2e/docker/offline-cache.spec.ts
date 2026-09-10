import { test, expect } from "@playwright/test";
import { TuiPage } from "./helpers/tui-page";
import { resetMock, setupMock } from "./helpers/mock-api-client";

test.beforeEach(async () => {
  await resetMock();
});

test.afterEach(async () => {
  // Never leave the shared mock in outage mode for the next spec.
  await resetMock();
});

/**
 * The reported bug: with no internet, a refresh made Linear ticket data vanish
 * (so the list "went ungrouped") and PR statuses disappear. Cached data must
 * survive the outage, with a hint in the action bar saying it isn't fresh.
 */
test("keeps Linear ticket data on screen when the API goes down", async ({ page }) => {
  const tui = new TuiPage(page);
  await tui.goto();
  await tui.waitForText("main", 10_000);

  // Online first, so there is something worth caching.
  await tui.waitForText("ENG-123", 15_000);

  await setupMock({ outage: true });
  await tui.sendKey("r");

  // The hint appears...
  await tui.waitForText("cached", 15_000);

  // ...and the ticket is still there rather than blanked out.
  const text = await tui.getScreenText();
  expect(text).toContain("ENG-123");

  await tui.screenshot("offline-cached-linear");
});

test("keeps PR status on screen when gh cannot reach GitHub", async ({ page }) => {
  const tui = new TuiPage(page);
  await tui.goto();
  await tui.waitForText("main", 10_000);
  await tui.waitForText("ENG-123", 15_000);

  const online = await tui.getScreenText();
  expect(online).toContain("Review");

  await setupMock({ outage: true });
  await tui.sendKey("r");
  await tui.waitForText("cached", 15_000);

  const offline = await tui.getScreenText();
  expect(offline).toContain("Review");

  await tui.screenshot("offline-cached-pr");
});

test("clears the cached hint once the API recovers", async ({ page }) => {
  const tui = new TuiPage(page);
  await tui.goto();
  await tui.waitForText("main", 10_000);
  await tui.waitForText("ENG-123", 15_000);

  await setupMock({ outage: true });
  await tui.sendKey("r");
  await tui.waitForText("cached", 15_000);

  await setupMock({ outage: false });
  await tui.sendKey("r");

  await expect
    .poll(async () => (await tui.getScreenText()).includes("cached"), { timeout: 20_000 })
    .toBe(false);

  expect(await tui.getScreenText()).toContain("ENG-123");
  await tui.screenshot("offline-cache-recovered");
});
