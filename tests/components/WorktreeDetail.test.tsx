import React from "react";
import { describe, it, expect, vi } from "vitest";
import { render } from "ink-testing-library";
import { WorktreeDetail } from "../../src/components/WorktreeDetail.js";
import type { WorktreeWithStatus } from "../../src/lib/types.js";

vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

function makeWorktree(overrides: Partial<WorktreeWithStatus> = {}): WorktreeWithStatus {
  return {
    id: "wt-1",
    repo_id: "repo-1",
    path: "/tmp/wt",
    branch: "feature/test",
    name: "test",
    custom_name: null,
    is_main: 0,
    created_at: "2024-01-01",
    agent_status: null,
    git_status: null,
    last_commit: null,
    pr_info: null,
    linear_info: null,
    ...overrides,
  };
}

describe("WorktreeDetail", () => {
  it("shows empty state when no worktree", () => {
    const { lastFrame } = render(<WorktreeDetail worktree={null} />);
    expect(lastFrame()!).toContain("Select a worktree");
  });

  it("shows branch name", () => {
    const { lastFrame } = render(
      <WorktreeDetail worktree={makeWorktree()} />
    );
    expect(lastFrame()!).toContain("feature/test");
  });

  it("shows agent status", () => {
    const wt = makeWorktree({
      agent_status: {
        worktree_id: "wt-1",
        status: "executing",
        last_response: null,
        transcript_summary: null,
        session_id: null,
        is_open: 1,
        updated_at: new Date().toISOString(),
      },
    });
    const { lastFrame } = render(<WorktreeDetail worktree={wt} />);
    expect(lastFrame()!).toContain("Executing");
  });

  it("shows no active session when not open", () => {
    const wt = makeWorktree({
      agent_status: {
        worktree_id: "wt-1",
        status: "idle",
        last_response: "Done with task",
        transcript_summary: null,
        session_id: null,
        is_open: 0,
        updated_at: "2024-01-01",
      },
    });
    const { lastFrame } = render(<WorktreeDetail worktree={wt} />);
    expect(lastFrame()!).toContain("No active session");
    expect(lastFrame()!).toContain("Done with task");
  });

  it("shows Cursor open when open_ide is cursor", () => {
    const wt = makeWorktree({ open_ide: "cursor" });
    const { lastFrame } = render(<WorktreeDetail worktree={wt} />);
    expect(lastFrame()!).toContain("Cursor open");
  });

  it("shows VS Code open when open_ide is vscode", () => {
    const wt = makeWorktree({ open_ide: "vscode" });
    const { lastFrame } = render(<WorktreeDetail worktree={wt} />);
    expect(lastFrame()!).toContain("VS Code open");
  });

  it("shows both IDE and Shell when both are active", () => {
    const wt = makeWorktree({ open_ide: "cursor", has_terminal: true });
    const { lastFrame } = render(<WorktreeDetail worktree={wt} />);
    const frame = lastFrame()!;
    expect(frame).toContain("Cursor open");
    expect(frame).toContain("Shell open");
  });

  it("shows Shell open when only terminal is active", () => {
    const wt = makeWorktree({ has_terminal: true });
    const { lastFrame } = render(<WorktreeDetail worktree={wt} />);
    expect(lastFrame()!).toContain("Shell open");
    expect(lastFrame()!).not.toContain("Cursor");
    expect(lastFrame()!).not.toContain("VS Code");
  });

  it("shows transcript_summary as Task when executing", () => {
    const wt = makeWorktree({
      agent_status: {
        worktree_id: "wt-1",
        status: "executing",
        last_response: "old response",
        transcript_summary: "Working on feature X",
        session_id: null,
        is_open: 1,
        updated_at: new Date().toISOString(),
      },
    });
    const { lastFrame } = render(<WorktreeDetail worktree={wt} />);
    expect(lastFrame()!).toContain("Task");
    expect(lastFrame()!).toContain("Working on feature X");
  });

  it("shows Done status label when done", () => {
    const wt = makeWorktree({
      agent_status: {
        worktree_id: "wt-1",
        status: "done",
        last_response: "Finished implementing feature",
        transcript_summary: null,
        session_id: null,
        is_open: 1,
        updated_at: new Date().toISOString(),
      },
    });
    const { lastFrame } = render(<WorktreeDetail worktree={wt} />);
    expect(lastFrame()!).toContain("Done");
    expect(lastFrame()!).toContain("Finished implementing feature");
  });

  it("shows git status", () => {
    const wt = makeWorktree({
      git_status: { ahead: 2, behind: 1, dirty: 3 },
    });
    const { lastFrame } = render(<WorktreeDetail worktree={wt} />);
    const frame = lastFrame()!;
    expect(frame).toContain("↑2");
    expect(frame).toContain("↓1");
    expect(frame).toContain("3 dirty");
  });

  it("shows last commit", () => {
    const wt = makeWorktree({
      last_commit: { hash: "abc1234", message: "fix bug", relative_time: "5m ago" },
    });
    const { lastFrame } = render(<WorktreeDetail worktree={wt} />);
    expect(lastFrame()!).toContain("fix bug");
    expect(lastFrame()!).toContain("5m ago");
  });

  it("shows branch-only label for main worktree on feature branch", () => {
    const wt = makeWorktree({ is_main: 1, branch: "feature/test" });
    const { lastFrame } = render(<WorktreeDetail worktree={wt} />);
    expect(lastFrame()!).toContain("branch in main working tree");
  });

  it("shows main working tree label for main worktree on main branch", () => {
    const wt = makeWorktree({ is_main: 1, branch: "main" });
    const { lastFrame } = render(<WorktreeDetail worktree={wt} />);
    expect(lastFrame()!).toContain("main working tree");
    expect(lastFrame()!).not.toContain("branch in");
  });

  it("shows PR info", () => {
    const wt = makeWorktree({
      pr_info: {
        number: 42,
        title: "My PR",
        url: "https://github.com/test/test/pull/42",
        state: "OPEN",
        isDraft: false,
        reviewDecision: "",
        hasFeedback: false,
        checksStatus: "passing",
        activeCheckUrl: null,
        activeCheckName: null,
        checksWaiting: false,
      },
    });
    const { lastFrame } = render(<WorktreeDetail worktree={wt} />);
    const frame = lastFrame()!;
    expect(frame).toContain("PR #42");
    expect(frame).toContain("My PR");
  });

  it("shows active check name for merged PR with running deployment", () => {
    const wt = makeWorktree({
      pr_info: {
        number: 99,
        title: "Deploy feature",
        url: "https://github.com/test/test/pull/99",
        state: "MERGED",
        isDraft: false,
        reviewDecision: "",
        hasFeedback: false,
        checksStatus: "pending",
        activeCheckUrl: "https://github.com/test/test/actions/runs/123",
        activeCheckName: "Deploy to Production",
        checksWaiting: false,
      },
    });
    const { lastFrame } = render(<WorktreeDetail worktree={wt} />);
    const frame = lastFrame()!;
    expect(frame).toContain("PR #99");
    expect(frame).toContain("Deploy to Production");
    expect(frame).toContain("running");
  });

  it("shows awaiting approval for merged PR with waiting check", () => {
    const wt = makeWorktree({
      pr_info: {
        number: 100,
        title: "Deploy with approval",
        url: "https://github.com/test/test/pull/100",
        state: "MERGED",
        isDraft: false,
        reviewDecision: "",
        hasFeedback: false,
        checksStatus: "pending",
        activeCheckUrl: "https://github.com/test/test/actions/runs/456",
        activeCheckName: "Deploy to Staging",
        checksWaiting: true,
      },
    });
    const { lastFrame } = render(<WorktreeDetail worktree={wt} />);
    const frame = lastFrame()!;
    expect(frame).toContain("Deploy to Staging");
    expect(frame).toContain("awaiting approval");
  });
});
