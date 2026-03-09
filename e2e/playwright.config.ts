import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "../tests/e2e/docker",
  outputDir: "../tests/e2e/tmp/test-results",
  use: {
    baseURL: process.env.TUI_URL || "http://localhost:7681",
    screenshot: "on",
  },
  timeout: 30_000,
  retries: 1,
  workers: 1,
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],
});
