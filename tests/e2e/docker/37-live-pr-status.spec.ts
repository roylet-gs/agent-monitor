import { test, expect } from "@playwright/test";
import { RustTui, resetMock, setupMock } from "./helpers/rust-tui";

// Live PR updates (`ghLiveUpdates`, on by default): the CI watch polls each
// open PR's check suites with conditional REST requests every 15 s and
// refetches a PR as soon as its CI bucket moves, instead of waiting for the
// 180 s GraphQL poll. The mock derives the check suites from the current gh
// fixture, so swapping the fixture is a CI run finishing on GitHub.

const pr = (status: string, conclusion: string | null) => ({
  number: 42,
  title: "feat: add dashboard feature",
  url: "https://github.com/test/repo/pull/42",
  state: "OPEN",
  isDraft: false,
  reviewDecision: "REVIEW_REQUIRED",
  statusCheckRollup: [{ status, conclusion, name: "build" }],
});

test.beforeEach(async () => {
  await resetMock();
});

test("CI finishing shows up long before the next PR poll", async ({ page }) => {
  test.setTimeout(90_000);
  await setupMock({ gh: pr("IN_PROGRESS", null) });
  const tui = new RustTui(page);
  await tui.open();
  await tui.selectRow("main");
  await tui.waitForMatch(/│▌.*#42◌/m, 15_000);

  const finished = Date.now();
  await setupMock({ gh: pr("COMPLETED", "SUCCESS") });
  await tui.waitForMatch(/│▌.*#42✓/m, 40_000);
  // The CI watch polls every 15 s; the PR poll would take 180 s.
  expect(Date.now() - finished).toBeLessThan(40_000);
  await tui.screenshot("live-ci-finished");

  // And a failure the same way.
  await setupMock({ gh: pr("COMPLETED", "FAILURE") });
  await tui.waitForMatch(/│▌.*#42✗/m, 40_000);
  await tui.screenshot("live-ci-failed");
});
