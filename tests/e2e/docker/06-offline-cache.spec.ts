import { test, expect } from "@playwright/test";
import { RustTui, resetMock, setupMock } from "./helpers/rust-tui";

test.beforeEach(async () => {
  await resetMock();
});

test.afterEach(async () => {
  await resetMock();
});

test("cached PR/Linear data survives an outage, with an offline hint that clears on recovery", async ({ page }) => {
  const tui = new RustTui(page);

  // Populate the cache while the APIs are up.
  await tui.open();
  await tui.waitForText("#42✓", 15_000);
  await tui.waitForText("ENG-123", 15_000);

  // Go offline and start a new am-tui process (fresh circuit breakers).
  await setupMock({ outage: true });
  await page.reload();
  await tui.open();
  await tui.waitForMatch(/⚠ (GitHub \+ Linear|GitHub|Linear) offline — cached/, 20_000);
  await tui.waitForMatch(/⚠ GitHub \+ Linear offline — cached/, 10_000);
  // The hint is right-aligned on the key bar (last row).
  expect(await tui.keyBar()).toMatch(/\? help.*⚠ GitHub \+ Linear offline — cached/);
  const text = await tui.getScreenText();
  expect(text).toContain("#42✓");
  expect(text).toContain("ENG-123");
  await tui.screenshot("offline-cached");

  // Recover: `r` bypasses the backoff and clears the hint.
  await resetMock();
  await tui.sendKey("r");
  await tui.waitForGone("offline — cached", 20_000);
  expect(await tui.getScreenText()).toContain("#42✓");
});
