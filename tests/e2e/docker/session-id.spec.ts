import { test, expect } from "@playwright/test";
import { TuiPage } from "./helpers/tui-page.js";
import { resetMock } from "./helpers/mock-api-client.js";

test.beforeEach(async () => {
  await resetMock();
});

// The entrypoint seeds a Claude session id on the main worktree via a
// UserPromptSubmit hook event (before setting the "delegating" status, which
// preserves the session_id). The detail panel should render a "Session" row
// showing the full session UUID under the Claude status row.
test("detail panel shows the Claude session id", async ({ page }) => {
  const tui = new TuiPage(page);
  await tui.goto();
  await tui.waitForText("main", 10_000);

  // The detail panel for the selected (main) worktree should show the
  // "Session" label and the seeded session UUID.
  await tui.waitForText("Session", 10_000);
  await tui.waitForText("3f2a91c8-7b4d-4e0a-9c1f-8d2e5a6b7c90", 10_000);

  const text = await tui.getScreenText();
  expect(text).toContain("Session");
  expect(text).toContain("3f2a91c8-7b4d-4e0a-9c1f-8d2e5a6b7c90");

  await tui.screenshot("session-id-detail");
});
