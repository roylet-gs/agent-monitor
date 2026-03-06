import React from "react";
import { describe, it, expect, vi } from "vitest";
import { render } from "ink-testing-library";
import { WorktreeList } from "../../src/components/WorktreeList.js";
import type { WorktreeWithStatus, WorktreeGroup, Repository } from "../../src/lib/types.js";

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
    created_at: "2024-01-01",
    agent_status: null,
    git_status: null,
    last_commit: null,
    pr_info: null,
    linear_info: null,
    ...overrides,
  };
}

function makeRepo(overrides: Partial<Repository> = {}): Repository {
  return {
    id: "repo-1",
    path: "/tmp/repo",
    name: "test-repo",
    last_used_at: "2024-01-01",
    ...overrides,
  };
}

describe("WorktreeList", () => {
  it("shows empty state when no worktrees", () => {
    const { lastFrame } = render(
      <WorktreeList
        groups={[]}
        flatWorktrees={[]}
        selectedIndex={0}
        unseenIds={new Set()}
        compactView={false}
      />
    );
    expect(lastFrame()!).toContain("No worktrees");
  });

  it("renders worktrees with selection indicator", () => {
    const wt1 = makeWorktree({ id: "wt-1", branch: "feature/a" });
    const wt2 = makeWorktree({ id: "wt-2", branch: "feature/b" });
    const group: WorktreeGroup = { repo: makeRepo(), worktrees: [wt1, wt2] };

    const { lastFrame } = render(
      <WorktreeList
        groups={[group]}
        flatWorktrees={[wt1, wt2]}
        selectedIndex={0}
        unseenIds={new Set()}
        compactView={false}
      />
    );
    const frame = lastFrame()!;
    expect(frame).toContain("feature/a");
    expect(frame).toContain("feature/b");
    expect(frame).toContain("▸"); // Selection indicator
  });

  it("shows unseen indicator", () => {
    const wt = makeWorktree({ id: "wt-1" });
    const group: WorktreeGroup = { repo: makeRepo(), worktrees: [wt] };

    const { lastFrame } = render(
      <WorktreeList
        groups={[group]}
        flatWorktrees={[wt]}
        selectedIndex={0}
        unseenIds={new Set(["wt-1"])}
        compactView={false}
      />
    );
    expect(lastFrame()!).toContain("*");
  });

  it("shows repo headers for multiple groups", () => {
    const repo1 = makeRepo({ id: "repo-1", name: "repo-one" });
    const repo2 = makeRepo({ id: "repo-2", name: "repo-two" });
    const wt1 = makeWorktree({ id: "wt-1", repo_id: "repo-1", branch: "feature/a" });
    const wt2 = makeWorktree({ id: "wt-2", repo_id: "repo-2", branch: "feature/b" });
    const groups: WorktreeGroup[] = [
      { repo: repo1, worktrees: [wt1] },
      { repo: repo2, worktrees: [wt2] },
    ];

    const { lastFrame } = render(
      <WorktreeList
        groups={groups}
        flatWorktrees={[wt1, wt2]}
        selectedIndex={0}
        unseenIds={new Set()}
        compactView={false}
      />
    );
    const frame = lastFrame()!;
    expect(frame).toContain("repo-one");
    expect(frame).toContain("repo-two");
  });

  it("shows filled dot for open session", () => {
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
    const group: WorktreeGroup = { repo: makeRepo(), worktrees: [wt] };

    const { lastFrame } = render(
      <WorktreeList
        groups={[group]}
        flatWorktrees={[wt]}
        selectedIndex={0}
        unseenIds={new Set()}
        compactView={false}
      />
    );
    expect(lastFrame()!).toContain("●");
  });

  it("shows outlined dot when no active session", () => {
    const wt = makeWorktree({ agent_status: null });
    const group: WorktreeGroup = { repo: makeRepo(), worktrees: [wt] };

    const { lastFrame } = render(
      <WorktreeList
        groups={[group]}
        flatWorktrees={[wt]}
        selectedIndex={0}
        unseenIds={new Set()}
        compactView={false}
      />
    );
    expect(lastFrame()!).toContain("○");
  });

  it("shows custom name with branch underneath", () => {
    const wt = makeWorktree({ custom_name: "My Feature", branch: "feature/custom" });
    const group: WorktreeGroup = { repo: makeRepo(), worktrees: [wt] };

    const { lastFrame } = render(
      <WorktreeList
        groups={[group]}
        flatWorktrees={[wt]}
        selectedIndex={0}
        unseenIds={new Set()}
        compactView={false}
      />
    );
    const frame = lastFrame()!;
    expect(frame).toContain("My Feature");
    expect(frame).toContain("feature/custom");
  });
});
