import { defineConfig, devices } from "@playwright/test";

// Playwright config for the Rust-binary E2E specs (tests/e2e/docker-rust/),
// run by the `tests-rust` compose service against `app-rust`.
export default defineConfig({
  testDir: "../tests/e2e/docker-rust",
  outputDir: "../tests/e2e/tmp/test-results-rust",
  use: {
    baseURL: process.env.TUI_URL || "http://localhost:7691",
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
