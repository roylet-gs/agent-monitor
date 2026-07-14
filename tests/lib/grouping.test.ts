import { describe, it, expect, vi } from "vitest";
import { buildGroups, compareWorktrees, type RepoWorktrees } from "../../src/lib/grouping.js";
import type { LinearProject, Repository, WorktreeWithStatus } from "../../src/lib/types.js";

vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

function makeRepo(overrides: Partial<Repository> = {}): Repository {
  return {
    id: "repo-1",
    path: "/tmp/repo",
    name: "test-repo",
    last_used_at: "2024-01-01",
    ...overrides,
  };
}

function makeWorktree(overrides: Partial<WorktreeWithStatus> = {}): WorktreeWithStatus {
  return {
    id: "wt-1",
    repo_id: "repo-1",
    path: "/tmp/wt",
    branch: "feature/test",
    name: "test",
    custom_name: null,
    nickname_source: null,
    is_main: 0,
    created_at: "2024-01-01",
    agent_status: null,
    git_status: null,
    last_commit: null,
    pr_info: null,
    linear_info: null,
    has_terminal: false,
    open_ide: null,
    ...overrides,
  };
}

function makeLinear(identifier: string, project: LinearProject | null = null) {
  return {
    identifier,
    title: `Ticket ${identifier}`,
    url: `https://linear.app/team/issue/${identifier}`,
    state: { name: "In Progress", color: "#0ea5e9", type: "started" },
    priorityLabel: "High",
    assignee: null,
    project,
  };
}

const PROJ_A: LinearProject = { id: "proj-a", name: "Alpha", color: "#5e6ad2" };
const PROJ_B: LinearProject = { id: "proj-b", name: "Beta" };

describe("compareWorktrees", () => {
  it("sorts dedicated worktrees before main, clusters tickets, newest first", () => {
    const main = makeWorktree({ id: "main", is_main: 1, created_at: "2024-01-05" });
    const eng1a = makeWorktree({ id: "eng1a", linear_info: makeLinear("ENG-1"), created_at: "2024-01-01" });
    const eng1b = makeWorktree({ id: "eng1b", linear_info: makeLinear("ENG-1"), created_at: "2024-01-03" });
    const plain = makeWorktree({ id: "plain", created_at: "2024-01-04" });

    const sorted = [main, plain, eng1a, eng1b].sort(compareWorktrees);
    expect(sorted.map((w) => w.id)).toEqual(["eng1b", "eng1a", "plain", "main"]);
  });
});

describe("buildGroups", () => {
  it("reproduces legacy per-repo layout when groupByProject is false", () => {
    const repo = makeRepo();
    const wt1 = makeWorktree({ id: "wt-1", created_at: "2024-01-02" });
    const wt2 = makeWorktree({ id: "wt-2", created_at: "2024-01-01" });
    const { groups, flatWorktrees } = buildGroups(
      [{ repo, worktrees: [wt2, wt1] }],
      { groupByProject: false }
    );
    expect(groups).toHaveLength(1);
    expect(groups[0].project).toBeUndefined();
    expect(groups[0].worktrees.map((w) => w.id)).toEqual(["wt-1", "wt-2"]);
    expect(flatWorktrees.map((w) => w.id)).toEqual(["wt-1", "wt-2"]);
  });

  it("keeps an empty group for a single repo (legacy rule)", () => {
    const { groups } = buildGroups([{ repo: makeRepo(), worktrees: [] }], { groupByProject: false });
    expect(groups).toHaveLength(1);
    expect(groups[0].worktrees).toEqual([]);
  });

  it("drops empty groups when multiple repos", () => {
    const perRepo: RepoWorktrees[] = [
      { repo: makeRepo({ id: "r1" }), worktrees: [] },
      { repo: makeRepo({ id: "r2" }), worktrees: [makeWorktree({ id: "wt-1", repo_id: "r2" })] },
    ];
    const { groups } = buildGroups(perRepo, { groupByProject: false });
    expect(groups).toHaveLength(1);
    expect(groups[0].repo.id).toBe("r2");
  });

  it("buckets worktrees under their project, remainder trailing", () => {
    const repo = makeRepo();
    const inProject = makeWorktree({ id: "in-proj", linear_info: makeLinear("ENG-1", PROJ_A) });
    const noProject = makeWorktree({ id: "no-proj", linear_info: makeLinear("ENG-2") });
    const noTicket = makeWorktree({ id: "no-ticket" });

    const { groups, flatWorktrees } = buildGroups(
      [{ repo, worktrees: [noTicket, inProject, noProject] }],
      { groupByProject: true }
    );

    expect(groups).toHaveLength(2);
    expect(groups[0].project).toEqual(PROJ_A);
    expect(groups[0].worktrees.map((w) => w.id)).toEqual(["in-proj"]);
    expect(groups[1].project).toBeUndefined();
    expect(groups[1].worktrees.map((w) => w.id)).toEqual(["no-proj", "no-ticket"]);
    // flat mirrors group order
    expect(flatWorktrees.map((w) => w.id)).toEqual(["in-proj", "no-proj", "no-ticket"]);
  });

  it("creates a project section even for a single worktree", () => {
    const repo = makeRepo();
    const wt = makeWorktree({ id: "only", linear_info: makeLinear("ENG-1", PROJ_A) });
    const { groups } = buildGroups([{ repo, worktrees: [wt] }], { groupByProject: true });
    expect(groups).toHaveLength(1);
    expect(groups[0].project?.id).toBe("proj-a");
  });

  it("orders project sections by name and repos in input order within a project", () => {
    const r1 = makeRepo({ id: "r1", name: "repo-one" });
    const r2 = makeRepo({ id: "r2", name: "repo-two" });
    const b1 = makeWorktree({ id: "b1", repo_id: "r1", linear_info: makeLinear("ENG-2", PROJ_B) });
    const a1 = makeWorktree({ id: "a1", repo_id: "r1", linear_info: makeLinear("ENG-1", PROJ_A) });
    const a2 = makeWorktree({ id: "a2", repo_id: "r2", linear_info: makeLinear("ENG-1", PROJ_A) });

    const { groups, flatWorktrees } = buildGroups(
      [
        { repo: r1, worktrees: [b1, a1] },
        { repo: r2, worktrees: [a2] },
      ],
      { groupByProject: true }
    );

    // Alpha (2 repo buckets in repo order) then Beta
    expect(groups.map((g) => [g.project?.name, g.repo.id])).toEqual([
      ["Alpha", "r1"],
      ["Alpha", "r2"],
      ["Beta", "r1"],
    ]);
    expect(flatWorktrees.map((w) => w.id)).toEqual(["a1", "a2", "b1"]);
  });

  it("clusters a cross-repo ticket under one project as separate repo buckets", () => {
    const r1 = makeRepo({ id: "r1" });
    const r2 = makeRepo({ id: "r2" });
    const wt1 = makeWorktree({ id: "wt1", repo_id: "r1", linear_info: makeLinear("ENG-1", PROJ_A) });
    const wt2 = makeWorktree({ id: "wt2", repo_id: "r2", linear_info: makeLinear("ENG-1", PROJ_A) });

    const { groups } = buildGroups(
      [
        { repo: r1, worktrees: [wt1] },
        { repo: r2, worktrees: [wt2] },
      ],
      { groupByProject: true }
    );
    expect(groups).toHaveLength(2);
    expect(groups.every((g) => g.project?.id === "proj-a")).toBe(true);
  });

  it("flat order always equals concatenated group order", () => {
    const r1 = makeRepo({ id: "r1" });
    const r2 = makeRepo({ id: "r2" });
    const wts1 = [
      makeWorktree({ id: "w1", repo_id: "r1", linear_info: makeLinear("ENG-1", PROJ_A) }),
      makeWorktree({ id: "w2", repo_id: "r1", is_main: 1 }),
      makeWorktree({ id: "w3", repo_id: "r1", linear_info: makeLinear("ENG-3", PROJ_B) }),
    ];
    const wts2 = [
      makeWorktree({ id: "w4", repo_id: "r2" }),
      makeWorktree({ id: "w5", repo_id: "r2", linear_info: makeLinear("ENG-1", PROJ_A) }),
    ];
    const { groups, flatWorktrees } = buildGroups(
      [
        { repo: r1, worktrees: wts1 },
        { repo: r2, worktrees: wts2 },
      ],
      { groupByProject: true }
    );
    expect(flatWorktrees).toEqual(groups.flatMap((g) => g.worktrees));
    expect(flatWorktrees).toHaveLength(5);
  });

  it("keeps the single-repo empty-group rule only when nothing rendered above", () => {
    const repo = makeRepo();
    const wt = makeWorktree({ id: "only", linear_info: makeLinear("ENG-1", PROJ_A) });
    // All worktrees consumed by the project section -> no empty trailing group
    const { groups } = buildGroups([{ repo, worktrees: [wt] }], { groupByProject: true });
    expect(groups).toHaveLength(1);
    // No worktrees at all -> keep the empty group so the dashboard shows the repo
    const empty = buildGroups([{ repo, worktrees: [] }], { groupByProject: true });
    expect(empty.groups).toHaveLength(1);
    expect(empty.groups[0].worktrees).toEqual([]);
  });
});
