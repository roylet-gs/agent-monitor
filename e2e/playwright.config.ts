import { defineConfig, devices } from "@playwright/test";

// Playwright config for the E2E specs (tests/e2e/docker/), run by the
// `tests` compose service against `app` (am-tui behind ttyd).
export default defineConfig({
  testDir: "../tests/e2e/docker",
  outputDir: "../tests/e2e/tmp/test-results",
  use: {
    baseURL: process.env.TUI_URL || "http://localhost:7681",
    screenshot: "on",
  },
  timeout: 45_000,
  retries: 1,
  // One shared seeded TUI (DB + settings.json) — specs must run sequentially.
  workers: 1,
  fullyParallel: false,
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"], viewport: { width: 1280, height: 1080 } },
    },
  ],
});
