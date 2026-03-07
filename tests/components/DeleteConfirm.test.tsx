import React from "react";
import { describe, it, expect, vi } from "vitest";
import { render } from "ink-testing-library";
import { DeleteConfirm } from "../../src/components/DeleteConfirm.js";
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

const ESCAPE = "\u001B";
const ENTER = "\r";

function waitForFrame(ms = 50): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}

describe("DeleteConfirm", () => {
  it("renders delete confirmation with branch name", () => {
    const { lastFrame } = render(
      <DeleteConfirm
        worktree={makeWorktree()}
        repoPath="/tmp/repo"
        onConfirm={vi.fn()}
        onCancel={vi.fn()}
      />
    );
    const frame = lastFrame()!;
    expect(frame).toContain("feature/test");
    expect(frame).toContain("Remove this worktree");
  });

  it("shows custom name if set", () => {
    const { lastFrame } = render(
      <DeleteConfirm
        worktree={makeWorktree({ custom_name: "My Feature" })}
        repoPath="/tmp/repo"
        onConfirm={vi.fn()}
        onCancel={vi.fn()}
      />
    );
    expect(lastFrame()!).toContain("My Feature");
  });

  it("shows warnings for dirty worktree", () => {
    const { lastFrame } = render(
      <DeleteConfirm
        worktree={makeWorktree({ git_status: { ahead: 2, behind: 0, dirty: 3 } })}
        repoPath="/tmp/repo"
        onConfirm={vi.fn()}
        onCancel={vi.fn()}
      />
    );
    const frame = lastFrame()!;
    expect(frame).toContain("3 uncommitted");
    expect(frame).toContain("2 commit");
  });

  it("calls onCancel on Escape", async () => {
    const onCancel = vi.fn();
    const { stdin } = render(
      <DeleteConfirm
        worktree={makeWorktree()}
        repoPath="/tmp/repo"
        onConfirm={vi.fn()}
        onCancel={onCancel}
      />
    );
    await waitForFrame();
    stdin.write(ESCAPE);
    expect(onCancel).toHaveBeenCalled();
  });

  it("advances to local-branch step on Enter", async () => {
    const { stdin, lastFrame } = render(
      <DeleteConfirm
        worktree={makeWorktree()}
        repoPath="/tmp/repo"
        onConfirm={vi.fn()}
        onCancel={vi.fn()}
      />
    );
    await waitForFrame();
    stdin.write(ENTER);
    expect(lastFrame()!).toContain("delete local branch");
  });

  it("confirms with deleteLocalBranch true on Enter+Enter", async () => {
    const onConfirm = vi.fn();
    const { stdin } = render(
      <DeleteConfirm
        worktree={makeWorktree()}
        repoPath="/tmp/repo"
        onConfirm={onConfirm}
        onCancel={vi.fn()}
      />
    );
    await waitForFrame();
    stdin.write(ENTER); // confirm delete
    await waitForFrame();
    stdin.write(ENTER); // confirm delete local branch (Enter = y)
    expect(onConfirm).toHaveBeenCalledWith({
      deleteLocalBranch: true,
      deleteRemoteBranch: false,
    });
  });

  it("shows branch-only confirmation for is_main worktree on feature branch", () => {
    const { lastFrame } = render(
      <DeleteConfirm
        worktree={makeWorktree({ is_main: 1, branch: "feature/test" })}
        repoPath="/tmp/repo"
        onConfirm={vi.fn()}
        onCancel={vi.fn()}
      />
    );
    const frame = lastFrame()!;
    expect(frame).toContain('Delete branch');
    expect(frame).toContain("switch back to the default branch");
    expect(frame).not.toContain("Remove this worktree");
  });

  it("branch-only confirms with isBranchOnly on Enter", async () => {
    const onConfirm = vi.fn();
    const { stdin } = render(
      <DeleteConfirm
        worktree={makeWorktree({ is_main: 1, branch: "feature/test" })}
        repoPath="/tmp/repo"
        onConfirm={onConfirm}
        onCancel={vi.fn()}
      />
    );
    await waitForFrame();
    stdin.write(ENTER);
    expect(onConfirm).toHaveBeenCalledWith({
      deleteLocalBranch: true,
      deleteRemoteBranch: false,
      isBranchOnly: true,
    });
  });

  it("does not show branch-only for is_main on main branch", () => {
    const { lastFrame } = render(
      <DeleteConfirm
        worktree={makeWorktree({ is_main: 1, branch: "main" })}
        repoPath="/tmp/repo"
        onConfirm={vi.fn()}
        onCancel={vi.fn()}
      />
    );
    const frame = lastFrame()!;
    expect(frame).toContain("Remove this worktree");
  });

  it("confirms with deleteLocalBranch false on Enter+n", async () => {
    const onConfirm = vi.fn();
    const { stdin } = render(
      <DeleteConfirm
        worktree={makeWorktree()}
        repoPath="/tmp/repo"
        onConfirm={onConfirm}
        onCancel={vi.fn()}
      />
    );
    await waitForFrame();
    stdin.write(ENTER); // confirm delete
    await waitForFrame();
    stdin.write("n"); // keep local branch
    expect(onConfirm).toHaveBeenCalledWith({
      deleteLocalBranch: false,
      deleteRemoteBranch: false,
    });
  });
});
