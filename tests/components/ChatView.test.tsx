import React from "react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render } from "ink-testing-library";
import { appendFileSync, mkdirSync } from "fs";
import { join } from "path";
import type { Settings, Worktree, WorktreeWithStatus } from "../../src/lib/types.js";

vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

vi.mock("../../src/lib/pubsub-client.js", () => ({
  publishMessage: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../../src/lib/ide-launcher.js", () => ({
  openClaudeInTerminal: vi.fn(),
  openInIde: vi.fn(),
}));

const spawnMock = vi.fn();
vi.mock("child_process", async (importOriginal) => {
  const actual = await importOriginal<typeof import("child_process")>();
  return { ...actual, spawn: (...args: unknown[]) => spawnMock(...args) };
});

const ESCAPE = "\u001B";
const SHIFT_TAB = "\u001B[Z";
const SETTINGS = { ide: "cursor", agentPermissionMode: "acceptEdits", agentClaudeArgs: "" } as Settings;

function withStatus(wt: Worktree): WorktreeWithStatus {
  return {
    ...wt,
    agent_status: null,
    git_status: null,
    last_commit: null,
    pr_info: null,
    linear_info: null,
    has_terminal: false,
    open_ide: null,
  };
}

function waitForFrame(ms = 50): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}

describe("ChatView", () => {
  let ChatView: typeof import("../../src/components/ChatView.js").ChatView;
  let db: typeof import("../../src/lib/db.js");
  let cs: typeof import("../../src/lib/claude-session.js");
  let worktree: WorktreeWithStatus;

  beforeEach(async () => {
    spawnMock.mockReset().mockReturnValue({ pid: 4242, unref: vi.fn(), on: vi.fn() });
    ({ ChatView } = await import("../../src/components/ChatView.js"));
    db = await import("../../src/lib/db.js");
    cs = await import("../../src/lib/claude-session.js");
    const repo = db.addRepository("/tmp/am-test-repo", "test-repo");
    worktree = withStatus(db.upsertWorktree(repo.id, "/tmp/am-test-repo/wt", "feature/x", "wt"));
  });

  it("renders the empty state when no session exists", () => {
    const { lastFrame } = render(
      <ChatView worktree={worktree} settings={SETTINGS} onBack={vi.fn()} />
    );
    const frame = lastFrame()!;
    expect(frame).toContain("Chat — feature/x");
    expect(frame).toContain("no session yet");
    expect(frame).toContain("Send a prompt");
  });

  it("renders an existing transcript", async () => {
    const { SESSIONS_DIR } = await import("../../src/lib/paths.js");
    const session = cs.startTurn(worktree, "fix the bug", SETTINGS);
    mkdirSync(SESSIONS_DIR, { recursive: true });
    appendFileSync(
      join(SESSIONS_DIR, `${session.id}.jsonl`),
      JSON.stringify({ type: "assistant", message: { content: [{ type: "text", text: "On it." }] } }) + "\n"
    );

    const { lastFrame } = render(
      <ChatView worktree={worktree} settings={SETTINGS} onBack={vi.fn()} />
    );
    await waitForFrame();
    const frame = lastFrame()!;
    expect(frame).toContain("❯ fix the bug");
    expect(frame).toContain("On it.");
    expect(frame).toContain("1 turn");
  });

  it("starts a turn when a prompt is submitted", async () => {
    const { stdin, lastFrame } = render(
      <ChatView worktree={worktree} settings={SETTINGS} onBack={vi.fn()} />
    );
    await waitForFrame();
    stdin.write("hello agent");
    await waitForFrame();
    stdin.write("\r");
    // transcript refresh happens on the next 500ms poll tick
    await waitForFrame(600);

    expect(spawnMock).toHaveBeenCalledOnce();
    const [cmd, args] = spawnMock.mock.calls[0]!;
    expect(cmd).toBe("claude");
    expect(args[args.length - 1]).toBe("hello agent");
    expect(lastFrame()).toContain("❯ hello agent");
  });

  it("opens the session in a terminal when Tab is pressed", async () => {
    const { openClaudeInTerminal } = await import("../../src/lib/ide-launcher.js");
    const session = cs.startTurn(worktree, "go", SETTINGS);
    const { stdin } = render(
      <ChatView worktree={worktree} settings={SETTINGS} onBack={vi.fn()} />
    );
    await waitForFrame();
    stdin.write("\t");
    await waitForFrame();
    expect(openClaudeInTerminal).toHaveBeenCalledWith(
      worktree.path,
      false,
      "feature/x",
      session.id
    );
  });

  it("embedded mode renders without its own hint bar (ActionBar owns the keys)", () => {
    const { lastFrame } = render(
      <ChatView worktree={worktree} settings={SETTINGS} embedded onBack={vi.fn()} />
    );
    const frame = lastFrame()!;
    expect(frame).toContain("Chat — feature/x");
    expect(frame).not.toContain("[Enter] Send");
  });

  it("opens the worktree in the configured IDE on Shift+Tab", async () => {
    const { openInIde } = await import("../../src/lib/ide-launcher.js");
    const { stdin } = render(
      <ChatView worktree={worktree} settings={SETTINGS} onBack={vi.fn()} />
    );
    await waitForFrame();
    stdin.write(SHIFT_TAB);
    await waitForFrame();
    expect(openInIde).toHaveBeenCalledWith(worktree.path, "cursor", "feature/x");
  });

  it("calls onBack when Esc is pressed", async () => {
    const onBack = vi.fn();
    const { stdin } = render(
      <ChatView worktree={worktree} settings={SETTINGS} onBack={onBack} />
    );
    await waitForFrame();
    stdin.write(ESCAPE);
    expect(onBack).toHaveBeenCalled();
  });
});
