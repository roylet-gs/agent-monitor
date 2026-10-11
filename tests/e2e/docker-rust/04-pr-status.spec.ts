import { test, expect } from "@playwright/test";
import { RustTui, resetMock, setupMock } from "./helpers/rust-tui";

// The mock answers every branch of the batched `gh api graphql` lookup with
// the current fixture, so every row carries the same PR.

test.beforeEach(async () => {
  await resetMock();
});

test("open PR with passing checks", async ({ page }) => {
  await setupMock({
    gh: {
      number: 42,
      title: "feat: add dashboard feature",
      url: "https://github.com/test/repo/pull/42",
      state: "OPEN",
      isDraft: false,
      reviewDecision: "REVIEW_REQUIRED",
      statusCheckRollup: [{ status: "COMPLETED", conclusion: "SUCCESS", name: "build" }],
    },
  });
  const tui = new RustTui(page);
  await tui.open();
  await tui.selectRow("main");
  await tui.waitForText("#42✓", 15_000);
  await tui.waitForMatch(/PR #42 open/, 5_000);
  await tui.screenshot("rust-pr-open");
});

test("draft PR", async ({ page }) => {
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
  const tui = new RustTui(page);
  await tui.open();
  await tui.selectRow("main");
  await tui.waitForText("#43", 15_000);
  await tui.waitForMatch(/PR #43 draft/, 5_000);
  await tui.screenshot("rust-pr-draft");
});

test("no PR on the branch", async ({ page }) => {
  // The default Linear fixture carries a GitHub PR attachment (#42), which the
  // app uses as a PR fallback when GitHub has none — so unlink the ticket too.
  await setupMock({ gh: null, linear: { data: { issueVcsBranchSearch: null } } });
  const tui = new RustTui(page);
  await tui.open();
  await tui.selectRow("main");
  // Whatever an earlier spec cached is replaced by the authoritative "no PR".
  await tui.waitForGone("PR #", 15_000);
  await tui.waitForGone("#4", 5_000);
  expect(await tui.contextLine()).not.toContain("PR #");
  await tui.screenshot("rust-pr-none");
});

test("Linear PR attachment stands in when GitHub has no PR", async ({ page }) => {
  await setupMock({ gh: null }); // default Linear fixture: attachment PR #42
  const tui = new RustTui(page);
  await tui.open();
  await tui.selectRow("main");
  await tui.waitForMatch(/PR #42 open/, 15_000);
  // No check status comes from an attachment, hence no ✓.
  await tui.waitForGone("#42✓", 15_000);
});
