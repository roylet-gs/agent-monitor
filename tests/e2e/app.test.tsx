import React from "react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render } from "ink-testing-library";
import { writeFileSync, mkdirSync } from "fs";

vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

vi.mock("../../src/lib/git.js", () => ({
  isGitRepo: vi.fn(() => false),
  getGitStatus: vi.fn().mockResolvedValue({ ahead: 0, behind: 0, dirty: 0 }),
  getLastCommit: vi.fn().mockResolvedValue(null),
  getMainBranch: vi.fn().mockResolvedValue("main"),
  branchExists: vi.fn().mockResolvedValue(false),
  createWorktree: vi.fn().mockResolvedValue("/tmp/wt"),
  getRepoName: vi.fn((p: string) => p.split("/").pop()),
  listWorktrees: vi.fn().mockResolvedValue([]),
  fetchBranch: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../../src/lib/github.js", () => ({
  fetchPrInfo: vi.fn().mockResolvedValue(null),
  fetchAllPrInfo: vi.fn().mockResolvedValue(new Map()),
  getPrStatusLabel: vi.fn(() => ({ label: "In Review", color: "cyan" })),
  isGhAvailable: vi.fn(() => false),
  deriveChecksStatus: vi.fn(() => "none"),
}));

vi.mock("../../src/lib/linear.js", () => ({
  fetchLinearInfo: vi.fn().mockResolvedValue(null),
  verifyLinearApiKey: vi.fn().mockResolvedValue({ ok: true }),
  getLinearStatusColor: vi.fn(() => "cyan"),
}));

vi.mock("../../src/lib/hooks-installer.js", () => ({
  installGlobalHooks: vi.fn(),
  isGlobalHooksInstalled: vi.fn(() => true),
}));

vi.mock("../../src/lib/ide-launcher.js", () => ({
  openInIde: vi.fn(),
}));

vi.mock("../../src/lib/sync.js", () => ({
  syncWorktrees: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../../src/lib/pubsub-server.js", () => ({
  startPubSubServer: vi.fn().mockResolvedValue(null),
  stopPubSubServer: vi.fn(),
}));

vi.mock("../../src/lib/pubsub-client.js", () => ({
  publishMessage: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../../src/lib/version.js", () => ({
  getVersion: vi.fn(() => "0.0.0-test"),
  isNewVersion: vi.fn(() => false),
  checkForUpdate: vi.fn().mockResolvedValue(null),
  detectPackageManager: vi.fn(() => ({ name: "npm", command: "npm update" })),
}));

vi.mock("../../src/lib/scripts.js", () => ({
  hasStartupScript: vi.fn(() => false),
  getScriptPath: vi.fn(() => "/tmp/script.sh"),
  createStartupScript: vi.fn(),
  openScriptInEditor: vi.fn(),
  removeStartupScript: vi.fn(),
}));

function waitForFrame(ms = 50): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}

describe("App E2E", () => {
  let App: typeof import("../../src/app.js").App;
  let paths: typeof import("../../src/lib/paths.js");

  beforeEach(async () => {
    paths = await import("../../src/lib/paths.js");
    ({ App } = await import("../../src/app.js"));
  });

  it("renders setup wizard on first run (no settings file)", async () => {
    const { lastFrame } = render(<App />);
    // useEffect runs async, wait for re-render
    await waitForFrame();
    expect(lastFrame()!).toContain("Welcome to Agent Monitor");
  });

  it("renders setup wizard when forceSetup is true", async () => {
    mkdirSync(paths.APP_DIR, { recursive: true });
    writeFileSync(paths.SETTINGS_PATH, JSON.stringify({ setupCompleted: true }));

    const { lastFrame } = render(<App forceSetup />);
    await waitForFrame();
    expect(lastFrame()!).toContain("Welcome to Agent Monitor");
  });

  it("renders dashboard when setup is completed and repos exist", async () => {
    mkdirSync(paths.APP_DIR, { recursive: true });
    writeFileSync(paths.SETTINGS_PATH, JSON.stringify({ setupCompleted: true }));

    const db = await import("../../src/lib/db.js");
    db.addRepository("/tmp/test-repo", "test-repo");

    const { lastFrame } = render(<App />);
    await waitForFrame();
    const frame = lastFrame()!;
    expect(frame).toContain("Agent Monitor");
    expect(frame).toContain("test-repo");
  });

  it("shows folder browser when no repos and setup completed", async () => {
    mkdirSync(paths.APP_DIR, { recursive: true });
    writeFileSync(paths.SETTINGS_PATH, JSON.stringify({ setupCompleted: true }));

    const { lastFrame } = render(<App />);
    await waitForFrame();
    // The folder browser should be active (no "Welcome" since setup is done)
    const frame = lastFrame()!;
    expect(frame).not.toContain("Welcome");
  });
});
