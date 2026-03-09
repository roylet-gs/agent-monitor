import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    globals: true,
    environment: "node",
    include: ["tests/**/*.test.{ts,tsx}"],
    exclude: ["tests/e2e/docker/**"],
    setupFiles: ["tests/setup.ts"],
    pool: "forks", // Required for better-sqlite3 native addon
    coverage: {
      provider: "v8",
      include: ["src/**"],
      exclude: ["src/cli.tsx"],
    },
  },
});
