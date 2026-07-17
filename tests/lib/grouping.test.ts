import { describe, it, expect, vi } from "vitest";
import { buildGroups, compareWorktrees, type RepoWorktrees } from "../../src/lib/grouping.js";
import { DEFAULT_SETTINGS } from "../../src/lib/settings.js";
import type { LinearProject, Repository, WorktreeWithStatus } from "../../src/lib/types.js";

const SORT = DEFAULT_SETTINGS.worktreeSort;

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
  it("produces one group per repo, sorted by the criteria", () => {
    const repo = makeRepo();
    const wt1 = makeWorktree({ id: "wt-1", created_at: "2024-01-02" });
    const wt2 = makeWorktree({ id: "wt-2", created_at: "2024-01-01" });
    const { groups, flatWorktrees } = buildGroups([{ repo, worktrees: [wt2, wt1] }], SORT);
    expect(groups).toHaveLength(1);
    expect(groups[0].worktrees.map((w) => w.id)).toEqual(["wt-1", "wt-2"]); // newest first
    expect(flatWorktrees.map((w) => w.id)).toEqual(["wt-1", "wt-2"]);
  });

  it("keeps an empty group for a single repo", () => {
    const { groups } = buildGroups([{ repo: makeRepo(), worktrees: [] }], SORT);
    expect(groups).toHaveLength(1);
    expect(groups[0].worktrees).toEqual([]);
  });

  it("drops empty groups when multiple repos", () => {
    const perRepo: RepoWorktrees[] = [
      { repo: makeRepo({ id: "r1" }), worktrees: [] },
      { repo: makeRepo({ id: "r2" }), worktrees: [makeWorktree({ id: "wt-1", repo_id: "r2" })] },
    ];
    const { groups } = buildGroups(perRepo, SORT);
    expect(groups).toHaveLength(1);
    expect(groups[0].repo.id).toBe("r2");
  });

  it("preserves repo input order across multiple repos", () => {
    const r1 = makeRepo({ id: "r1", name: "repo-one" });
    const r2 = makeRepo({ id: "r2", name: "repo-two" });
    const { groups } = buildGroups(
      [
        { repo: r1, worktrees: [makeWorktree({ id: "a", repo_id: "r1" })] },
        { repo: r2, worktrees: [makeWorktree({ id: "b", repo_id: "r2" })] },
      ],
      SORT
    );
    expect(groups.map((g) => g.repo.id)).toEqual(["r1", "r2"]);
  });

  it("orders repo sections by name when a repo criterion is enabled", () => {
    const web = makeRepo({ id: "r-web", name: "web-app" });
    const api = makeRepo({ id: "r-api", name: "agent-monitor" });
    const perRepo: RepoWorktrees[] = [
      { repo: web, worktrees: [makeWorktree({ id: "w", repo_id: "r-web" })] },
      { repo: api, worktrees: [makeWorktree({ id: "a", repo_id: "r-api" })] },
    ];
    const repoAsc = [
      { key: "repo" as const, direction: "asc" as const, enabled: true },
      ...SORT,
    ];
    const { groups } = buildGroups(perRepo, repoAsc);
    // agent-monitor sorts before web-app despite input order
    expect(groups.map((g) => g.repo.name)).toEqual(["agent-monitor", "web-app"]);

    const repoDesc = [
      { key: "repo" as const, direction: "desc" as const, enabled: true },
      ...SORT,
    ];
    const { groups: desc } = buildGroups(perRepo, repoDesc);
    expect(desc.map((g) => g.repo.name)).toEqual(["web-app", "agent-monitor"]);
  });

  it("keeps repo sections in input order when repo is not a criterion", () => {
    const web = makeRepo({ id: "r-web", name: "web-app" });
    const api = makeRepo({ id: "r-api", name: "agent-monitor" });
    const perRepo: RepoWorktrees[] = [
      { repo: web, worktrees: [makeWorktree({ id: "w", repo_id: "r-web" })] },
      { repo: api, worktrees: [makeWorktree({ id: "a", repo_id: "r-api" })] },
    ];
    const { groups } = buildGroups(perRepo, SORT);
    expect(groups.map((g) => g.repo.name)).toEqual(["web-app", "agent-monitor"]);
  });

  it("clusters worktrees sharing a Linear ticket adjacently via the sort", () => {
    const repo = makeRepo();
    const a = makeWorktree({ id: "a", linear_info: makeLinear("ENG-1", PROJ_A), created_at: "2024-01-01" });
    const b = makeWorktree({ id: "b", linear_info: makeLinear("ENG-1", PROJ_A), created_at: "2024-01-02" });
    const other = makeWorktree({ id: "other", linear_info: makeLinear("ENG-2"), created_at: "2024-01-03" });
    const { flatWorktrees } = buildGroups([{ repo, worktrees: [other, a, b] }], SORT);
    // ENG-1 pair adjacent (newest first within the ticket), then ENG-2
    expect(flatWorktrees.map((w) => w.id)).toEqual(["b", "a", "other"]);
  });

  it("clusters by Linear project when a linearProject criterion is enabled first", () => {
    const repo = makeRepo();
    const projB: LinearProject = { id: "proj-b", name: "Beta" };
    const a = makeWorktree({ id: "a", linear_info: makeLinear("ENG-9", PROJ_A) });
    const b = makeWorktree({ id: "b", linear_info: makeLinear("BUG-1", projB) });
    const a2 = makeWorktree({ id: "a2", linear_info: makeLinear("ENG-1", PROJ_A) });
    const criteria = [{ key: "linearProject" as const, direction: "asc" as const, enabled: true }];
    const { flatWorktrees } = buildGroups([{ repo, worktrees: [b, a, a2] }], criteria);
    // Alpha worktrees grouped before Beta — grouping "for free" via sort
    expect(flatWorktrees.map((w) => w.linear_info?.project?.name)).toEqual(["Alpha", "Alpha", "Beta"]);
  });

  it("flat order always equals concatenated group order", () => {
    const r1 = makeRepo({ id: "r1" });
    const r2 = makeRepo({ id: "r2" });
    const { groups, flatWorktrees } = buildGroups(
      [
        { repo: r1, worktrees: [makeWorktree({ id: "w1", repo_id: "r1" }), makeWorktree({ id: "w2", repo_id: "r1", is_main: 1 })] },
        { repo: r2, worktrees: [makeWorktree({ id: "w3", repo_id: "r2" })] },
      ],
      SORT
    );
    expect(flatWorktrees).toEqual(groups.flatMap((g) => g.worktrees));
    expect(flatWorktrees).toHaveLength(3);
  });
});
