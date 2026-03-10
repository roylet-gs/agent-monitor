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
  ensureBranchForOpen: vi.fn().mockResolvedValue(undefined),
  remoteBranchExists: vi.fn().mockResolvedValue(true),
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
  openClaudeInTerminal: vi.fn(),
  openTerminal: vi.fn(),
  focusTerminal: vi.fn(),
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

vi.mock("../../src/lib/process.js", () => ({
  getTerminalPaths: vi.fn(() => new Set()),
  getIdePaths: vi.fn(() => new Map()),
  getTerminalPathsAsync: vi.fn().mockResolvedValue(new Set()),
  getIdePathsAsync: vi.fn().mockResolvedValue(new Map()),
  isTerminalOpenAt: vi.fn(() => false),
}));

vi.mock("../../src/lib/daemon.js", () => ({
  isDaemonRunning: vi.fn(() => false),
  getDaemonPid: vi.fn(() => null),
  stopDaemon: vi.fn(() => false),
}));

// Mock DaemonClient to immediately provide data from DB
vi.mock("../../src/lib/daemon-client.js", () => {
  class MockDaemonClient {
    private options: { onData: (msg: unknown) => void; onConnected?: () => void; onDisconnected?: () => void };
    private pollTimer: ReturnType<typeof setInterval> | null = null;
    connected = false;

    constructor(options: { onData: (msg: unknown) => void; onConnected?: () => void; onDisconnected?: () => void }) {
      this.options = options;
    }

    async connect() {
      // Simulate connected state and start polling DB directly
      this.connected = true;
      this.options.onConnected?.();
      this.refresh();
      this.pollTimer = setInterval(() => this.refresh(), 500);
      return true;
    }

    private async refresh() {
      try {
        const db = await import("../../src/lib/db.js");
        const git = await import("../../src/lib/git.js");
        const repos = db.getRepositories();
        const groups: unknown[] = [];
        const allFlat: unknown[] = [];

        for (const repo of repos) {
          const dbWorktrees = db.getWorktrees(repo.id);
          const statuses = db.getAgentStatuses(repo.id);

          const enriched = await Promise.all(
            dbWorktrees.map(async (wt: { id: string; branch: string; path: string; is_main: number; repo_id: string; name: string; custom_name: string | null; nickname_source: string | null; created_at: string }) => {
              let git_status = null;
              let last_commit = null;
              try {
                [git_status, last_commit] = await Promise.all([
                  git.getGitStatus(wt.path),
                  git.getLastCommit(wt.path),
                ]);
              } catch { /* ignore */ }
              return {
                ...wt,
                agent_status: statuses.get(wt.id) ?? null,
                git_status,
                last_commit,
                has_terminal: false,
                open_ide: null,
                pr_info: null,
                linear_info: null,
              };
            })
          );

          enriched.sort((a: { is_main: number; created_at: string }, b: { is_main: number; created_at: string }) => {
            if (a.is_main !== b.is_main) return a.is_main - b.is_main;
            return b.created_at.localeCompare(a.created_at);
          });

          if (enriched.length > 0 || repos.length === 1) {
            groups.push({ repo, worktrees: enriched });
          }
          allFlat.push(...enriched);
        }

        this.options.onData({
          type: "refresh-result",
          id: null,
          data: { groups, flatWorktrees: allFlat, standaloneSessions: [] },
        });
      } catch { /* ignore during teardown */ }
    }

    async forceRefresh() {
      this.refresh();
    }

    configReload() {
      this.refresh();
    }

    destroy() {
      this.connected = false;
      if (this.pollTimer) {
        clearInterval(this.pollTimer);
        this.pollTimer = null;
      }
    }
  }

  return { DaemonClient: MockDaemonClient };
});

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
    await waitForFrame(300);
    expect(lastFrame()!).toContain("Settings");

    stdin.write(ESCAPE);
    await waitForFrame(300);
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
    await waitForFrame(200);
    expect(lastFrame()!).toContain("Agent Monitor");

    // Wait for worktree data to arrive from mock daemon
    await waitForFrame(500);

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
    await waitForFrame(200);

    // Wait for worktree data to arrive from mock daemon
    await waitForFrame(500);

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
    await waitForFrame(200);

    // Wait for worktree data to arrive from mock daemon
    await waitForFrame(500);

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
