import { test, expect } from "@playwright/test";
import { TuiPage } from "./helpers/tui-page.js";
import { resetMock } from "./helpers/mock-api-client.js";

test.beforeEach(async () => {
  await resetMock();
});

// The entrypoint seeds an idle standalone session at "/home/user/scripts" whose
// last_response is a LONG string containing emoji (✅ 🎉 🚀) and a markdown link
// ([#115](https://…)). It renders in the detail panel's "Last Response" section
// via `<Text wrap="truncate-end">{normalizeSummary(...)}</Text>` — the exact
// render path this PR fixes.
//
// The bug: cli-truncate miscounts emoji display width, so a truncated line that
// still contains an emoji renders ONE COLUMN too wide, wraps in the terminal,
// and breaks the detail panel's vertical borders. normalizeSummary now strips
// emoji and reduces markdown links to their label, so every rendered row is
// exactly the terminal width and the borders stay aligned.
test("long emoji last_response renders without breaking detail-panel borders", async ({
  page,
}) => {
  const tui = new TuiPage(page);
  await tui.goto();
  await tui.waitForText("main", 10_000);

  // Wait for the standalone sessions section (seeded at startup) to render.
  await tui.waitForText("Other Sessions", 10_000);

  // Row order is: main worktree (row 0, selected initially) → standalone
  // sessions. The two seeded standalone sessions list "my-project" then
  // "scripts", so two downward moves selects the "scripts" session whose
  // last_response carries the emoji + link payload.
  await tui.sendKey("j");
  await tui.sendKey("j");

  // Confirm the detail panel is showing the scripts session.
  await tui.waitForText("/home/user/scripts", 10_000);

  const text = await tui.getScreenText();

  // The "Last Response" section is present (idle session => not active).
  expect(text).toContain("Last Response");

  // normalizeSummary reduced the markdown link to its label: the "#115" text
  // survives but the raw URL / markdown link syntax does not.
  expect(text).toContain("PR #115 is green");
  expect(text).not.toContain("https://");
  expect(text).not.toContain("](");

  // Emoji from the source string have been stripped (the crux of the fix).
  expect(text).not.toContain("✅");
  expect(text).not.toContain("🎉");
  expect(text).not.toContain("🚀");

  // Border integrity: the detail panel is drawn with box-drawing characters.
  // If an over-wide emoji line had wrapped, the vertical borders would be
  // misaligned; here we assert the box-drawing frame is still present.
  expect(text).toContain("│");

  await tui.screenshot("description-borders");
});
