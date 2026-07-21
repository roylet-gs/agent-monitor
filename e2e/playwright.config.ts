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
      // Taller viewport → more xterm rows so the full-height Settings panel
      // fits without Ink overwriting lines (the default 720px clips it).
      use: { ...devices["Desktop Chrome"], viewport: { width: 1280, height: 1080 } },
    },
  ],
});
