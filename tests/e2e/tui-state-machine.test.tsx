import React from "react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render } from "ink-testing-library";
import { writeFileSync, mkdirSync, mkdtempSync } from "fs";
import { tmpdir } from "os";
import { join } from "path";

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
  deleteWorktree: vi.fn().mockResolvedValue(undefined),
  deleteBranch: vi.fn().mockResolvedValue(undefined),
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

const ESCAPE = "\u001B";

function waitForFrame(ms = 100): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}

describe("TUI State Machine", () => {
  let App: typeof import("../../src/app.js").App;
  let paths: typeof import("../../src/lib/paths.js");
  let db: typeof import("../../src/lib/db.js");

  async function setupDashboard() {
    const tempWtDir = mkdtempSync(join(tmpdir(), "am-e2e-"));
    mkdirSync(paths.APP_DIR, { recursive: true });
    writeFileSync(paths.SETTINGS_PATH, JSON.stringify({ setupCompleted: true }));
    const repo = db.addRepository("/tmp/repo", "test-repo");
    db.upsertWorktree(repo.id, tempWtDir, "feature/test", "test");
    return tempWtDir;
  }

  beforeEach(async () => {
    paths = await import("../../src/lib/paths.js");
    db = await import("../../src/lib/db.js");
    ({ App } = await import("../../src/app.js"));
  });

  it("dashboard -> settings -> back to dashboard", async () => {
    await setupDashboard();
    const { stdin, lastFrame } = render(<App />);
    await waitForFrame();
    expect(lastFrame()!).toContain("Agent Monitor");

    stdin.write("s");
    await waitForFrame();
    expect(lastFrame()!).toContain("Settings");

    stdin.write(ESCAPE);
    await waitForFrame();
    expect(lastFrame()!).toContain("Agent Monitor");
  });

  it("dashboard -> new worktree form -> cancel back to dashboard", async () => {
    await setupDashboard();
    const { stdin, lastFrame } = render(<App />);
    await waitForFrame();
    expect(lastFrame()!).toContain("Agent Monitor");

    stdin.write("n");
    await waitForFrame();
    expect(lastFrame()!).toContain("New Worktree");

    stdin.write(ESCAPE);
    await waitForFrame();
    expect(lastFrame()!).toContain("Agent Monitor");
  });

  it("dashboard -> delete confirm -> cancel back to dashboard", async () => {
    await setupDashboard();
    const { stdin, lastFrame } = render(<App />);
    await waitForFrame();
    expect(lastFrame()!).toContain("Agent Monitor");

    stdin.write("d");
    await waitForFrame();
    expect(lastFrame()!).toContain("Delete worktree");

    stdin.write("n");
    await waitForFrame();
    expect(lastFrame()!).toContain("Agent Monitor");
  });

  it("setup wizard -> skip -> exits wizard", async () => {
    const { stdin, lastFrame } = render(<App />);
    await waitForFrame();
    expect(lastFrame()!).toContain("Welcome");

    stdin.write(ESCAPE);
    await waitForFrame();
    expect(lastFrame()!).not.toContain("Welcome");
  });

  it("delete failure -> shows recovery prompt -> Esc cancels", async () => {
    const git = await import("../../src/lib/git.js");
    vi.mocked(git.deleteWorktree).mockRejectedValueOnce(new Error("is a main working tree"));

    await setupDashboard();
    const { stdin, lastFrame } = render(<App />);
    await waitForFrame();

    // Enter delete confirm
    stdin.write("d");
    await waitForFrame();
    expect(lastFrame()!).toContain("Delete worktree");

    // Confirm delete (Enter), then skip branch deletion (n)
    const ENTER = "\r";
    stdin.write(ENTER);
    await waitForFrame();
    stdin.write("n");

    // Wait for async delete to fail and recovery prompt to appear
    await waitForFrame(200);
    const frame = lastFrame()!;
    expect(frame).toContain("Delete Worktree");
    expect(frame).toContain("is a main working tree");
    expect(frame).toContain("Clean up");

    // Press Esc to cancel recovery
    stdin.write(ESCAPE);
    await waitForFrame();
    expect(lastFrame()!).toContain("Agent Monitor");
  });

  it("delete failure -> recovery prompt -> y cleans up DB", async () => {
    const git = await import("../../src/lib/git.js");
    vi.mocked(git.deleteWorktree).mockRejectedValueOnce(new Error("path does not exist"));

    await setupDashboard();
    const { stdin, lastFrame } = render(<App />);
    await waitForFrame();

    // Enter delete confirm
    stdin.write("d");
    await waitForFrame();

    // Confirm delete (Enter), then skip branch deletion (n)
    const ENTER = "\r";
    stdin.write(ENTER);
    await waitForFrame();
    stdin.write("n");

    // Wait for recovery prompt
    await waitForFrame(200);
    expect(lastFrame()!).toContain("path does not exist");
    expect(lastFrame()!).toContain("Clean up");

    // Press n to clean up DB only (no branch delete)
    stdin.write("n");
    await waitForFrame(800);

    // Should return to dashboard after recovery
    expect(lastFrame()!).toContain("Agent Monitor");
  });
});
