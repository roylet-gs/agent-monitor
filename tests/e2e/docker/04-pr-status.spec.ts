import { test, expect } from "@playwright/test";
import { RustTui, resetMock, setupMock } from "./helpers/rust-tui";

// The mock answers every branch of the batched `gh api graphql` lookup with
// the current fixture, so every row carries the same PR. PR info lives only in
// the row's PR column now (`#42✓`); state/draft is conveyed by colour alone.

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
  expect(await tui.selectedRow()).toContain("#42✓");
  // REVIEW_REQUIRED → `review` after the checks glyph.
  await tui.waitForMatch(/│▌.*#42✓ review/m, 15_000);
  await tui.screenshot("pr-open");
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
  // Draft = dim number (not visible in text); no checks → no glyph.
  await tui.waitForText("#43", 15_000);
  await tui.waitForGone("#42", 15_000);
  expect(await tui.selectedRow()).toMatch(/#43(\s|$)/);
  await tui.screenshot("pr-draft");
});

test("no PR on the branch", async ({ page }) => {
  // The default Linear fixture carries a GitHub PR attachment (#42), which the
  // app uses as a PR fallback when GitHub has none — so unlink the ticket too.
  await setupMock({ gh: null, linear: { data: { issueVcsBranchSearch: null } } });
  const tui = new RustTui(page);
  await tui.open();
  await tui.selectRow("main");
  // Whatever an earlier spec cached is replaced by the authoritative "no PR".
  await tui.waitForGone("#4", 15_000);
  expect(await tui.selectedRow()).not.toMatch(/#\d/);
  await tui.screenshot("pr-none");
});

test("Linear PR attachment stands in when GitHub has no PR", async ({ page }) => {
  await setupMock({ gh: null }); // default Linear fixture: attachment PR #42
  const tui = new RustTui(page);
  await tui.open();
  await tui.selectRow("main");
  // No check status comes from an attachment, hence #42 without ✓.
  await tui.waitForGone("#42✓", 15_000);
  await tui.waitForMatch(/│▌.*#42(\s|$)/m, 15_000);
  expect(await tui.selectedRow()).toMatch(/#42(\s|$)/);
});

// Review state after `#N` + checks glyph. The browser is 1280px wide, so the
// PR column has room for words (`ready`, `approved`, `changes`…) not glyphs.

test("approved PR with passing checks reads ready", async ({ page }) => {
  await setupMock({
    gh: {
      number: 42,
      title: "feat: add dashboard feature",
      url: "https://github.com/test/repo/pull/42",
      state: "OPEN",
      isDraft: false,
      reviewDecision: "APPROVED",
      mergeable: "MERGEABLE",
      statusCheckRollup: [{ status: "COMPLETED", conclusion: "SUCCESS", name: "build" }],
    },
  });
  const tui = new RustTui(page);
  await tui.open();
  await tui.selectRow("main");
  await tui.waitForMatch(/│▌.*#42✓ ready/m, 15_000);
  expect(await tui.selectedRow()).toContain("#42✓ ready");
  await tui.screenshot("pr-ready");
});

test("approved PR with a failing check reads approved", async ({ page }) => {
  await setupMock({
    gh: {
      number: 42,
      title: "feat: add dashboard feature",
      url: "https://github.com/test/repo/pull/42",
      state: "OPEN",
      isDraft: false,
      reviewDecision: "APPROVED",
      mergeable: "MERGEABLE",
      statusCheckRollup: [
        { status: "COMPLETED", conclusion: "SUCCESS", name: "build" },
        { status: "COMPLETED", conclusion: "FAILURE", name: "test" },
      ],
    },
  });
  const tui = new RustTui(page);
  await tui.open();
  await tui.selectRow("main");
  await tui.waitForMatch(/│▌.*#42✗ approved/m, 15_000);
  expect(await tui.selectedRow()).toContain("#42✗ approved");
  await tui.screenshot("pr-approved-checks-failing");
});

test("changes requested PR reads changes", async ({ page }) => {
  await setupMock({
    gh: {
      number: 42,
      title: "feat: add dashboard feature",
      url: "https://github.com/test/repo/pull/42",
      state: "OPEN",
      isDraft: false,
      reviewDecision: "CHANGES_REQUESTED",
      statusCheckRollup: [{ status: "COMPLETED", conclusion: "SUCCESS", name: "build" }],
    },
  });
  const tui = new RustTui(page);
  await tui.open();
  await tui.selectRow("main");
  await tui.waitForMatch(/│▌.*#42✓ changes/m, 15_000);
  expect(await tui.selectedRow()).toContain("#42✓ changes");
  await tui.screenshot("pr-changes-requested");
});
