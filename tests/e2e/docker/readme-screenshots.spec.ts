import { test } from "@playwright/test";
import { TuiPage } from "./helpers/tui-page.js";
import { setupMock, resetMock } from "./helpers/mock-api-client.js";

const LINEAR_NOT_FOUND = { data: { issueVcsBranchSearch: null } };

test.beforeEach(async () => {
  await resetMock();
});

test("readme-dashboard: clean dashboard without PR", async ({ page }) => {
  await setupMock({ gh: null, linear: LINEAR_NOT_FOUND });

  const tui = new TuiPage(page);
  await tui.goto();
  await tui.waitForText("main", 10_000);
  await page.waitForTimeout(3000);

  await tui.screenshot("readme-dashboard");
});

test("readme-dashboard-pr: hero image with PR + checks + Linear", async ({ page }) => {
  await setupMock({
    gh: {
      number: 42,
      title: "feat: add OAuth login flow",
      url: "https://github.com/acme/web-app/pull/42",
      state: "OPEN",
      isDraft: false,
      reviewDecision: "APPROVED",
      statusCheckRollup: [
        { status: "COMPLETED", conclusion: "SUCCESS", name: "build" },
        { status: "COMPLETED", conclusion: "SUCCESS", name: "test" },
        { status: "COMPLETED", conclusion: "SUCCESS", name: "lint" },
      ],
    },
    linear: {
      data: {
        issueVcsBranchSearch: {
          identifier: "ENG-123",
          title: "OAuth login flow for enterprise SSO",
          url: "https://linear.app/acme/issue/ENG-123",
          state: {
            name: "In Progress",
            color: "#0ea5e9",
            type: "started",
          },
          priorityLabel: "High",
          assignee: {
            name: "Alice Chen",
          },
          attachments: {
            nodes: [
              {
                url: "https://github.com/acme/web-app/pull/42",
                title: "feat: add OAuth login flow",
                sourceType: "github",
                metadata: {
                  number: 42,
                  draft: false,
                  mergedAt: null,
                  closedAt: null,
                },
              },
            ],
          },
        },
      },
    },
  });

  const tui = new TuiPage(page);
  await tui.goto();
  await tui.waitForText("main", 10_000);
  await page.waitForTimeout(5000);

  await tui.screenshot("readme-dashboard-pr");
});

test("readme-pr-draft: draft PR without checks", async ({ page }) => {
  await setupMock({
    gh: {
      number: 43,
      title: "feat: migrate to new payment provider",
      url: "https://github.com/acme/web-app/pull/43",
      state: "OPEN",
      isDraft: true,
      reviewDecision: "",
      statusCheckRollup: [],
    },
    linear: LINEAR_NOT_FOUND,
  });

  const tui = new TuiPage(page);
  await tui.goto();
  await tui.waitForText("main", 10_000);
  await page.waitForTimeout(3000);

  await tui.screenshot("readme-pr-draft");
});

test("readme-linear-ticket: Linear ticket without PR", async ({ page }) => {
  await setupMock({
    gh: null,
    linear: {
      data: {
        issueVcsBranchSearch: {
          identifier: "ENG-456",
          title: "Implement rate limiting for public API",
          url: "https://linear.app/acme/issue/ENG-456",
          state: {
            name: "In Progress",
            color: "#f59e0b",
            type: "started",
          },
          priorityLabel: "Urgent",
          assignee: {
            name: "Alice Chen",
          },
          attachments: {
            nodes: [],
          },
        },
      },
    },
  });

  const tui = new TuiPage(page);
  await tui.goto();
  await tui.waitForText("main", 10_000);
  await page.waitForTimeout(5000);

  await tui.screenshot("readme-linear-ticket");
});

test("readme-settings: settings screen", async ({ page }) => {
  const tui = new TuiPage(page);
  await tui.goto();
  await tui.waitForText("main", 10_000);

  await tui.sendKey("s");
  await page.waitForTimeout(1000);

  await tui.screenshot("readme-settings");
});

test("readme-new-worktree: new worktree form", async ({ page }) => {
  const tui = new TuiPage(page);
  await tui.goto();
  await tui.waitForText("main", 10_000);

  await tui.sendKey("n");
  await page.waitForTimeout(1000);

  await tui.screenshot("readme-new-worktree");
});

test("readme-standalone-session: standalone agent sessions", async ({ page }) => {
  const tui = new TuiPage(page);
  await tui.goto();
  await tui.waitForText("main", 10_000);

  // Navigate down to the standalone sessions section
  await tui.sendKey("j");
  await page.waitForTimeout(500);
  await tui.sendKey("j");
  await page.waitForTimeout(500);

  await tui.screenshot("readme-standalone-session");
});
