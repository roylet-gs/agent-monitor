import { describe, it, expect, vi } from "vitest";
import { makeComparator, applyWorktreeFilters, compareWorktrees } from "../../src/lib/grouping.js";
import { DEFAULT_SETTINGS } from "../../src/lib/settings.js";
import type {
  AgentStatus,
  AgentStatusType,
  PrInfo,
  WorktreeSortCriterion,
  WorktreeWithStatus,
} from "../../src/lib/types.js";

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

function status(status: AgentStatusType, updated_at = "2024-01-01", is_open = 0): AgentStatus {
  return {
    worktree_id: "wt-1",
    status,
    last_response: null,
    transcript_summary: null,
    session_id: null,
    is_open,
    updated_at,
  };
}

function pr(overrides: Partial<PrInfo> = {}): PrInfo {
  return {
    number: 1,
    title: "PR",
    url: "https://example.com/pr/1",
    state: "OPEN",
    isDraft: false,
    reviewDecision: "",
    hasFeedback: false,
    checksStatus: "none",
    activeCheckUrl: null,
    activeCheckName: null,
    checksWaiting: false,
    ...overrides,
  };
}

function only(key: WorktreeSortCriterion["key"], direction: "asc" | "desc" = "asc"): WorktreeSortCriterion[] {
  return [{ key, direction, enabled: true }];
}

describe("makeComparator", () => {
  it("orders agent status active-first (executing < ... < none)", () => {
    const exec = makeWorktree({ id: "exec", agent_status: status("executing") });
    const idle = makeWorktree({ id: "idle", agent_status: status("idle") });
    const none = makeWorktree({ id: "none", agent_status: null });
    const sorted = [none, idle, exec].sort(makeComparator(only("agentStatus")));
    expect(sorted.map((w) => w.id)).toEqual(["exec", "idle", "none"]);
  });

  it("orders PRs attention-first (failing before open before merged before none)", () => {
    const failing = makeWorktree({ id: "failing", pr_info: pr({ checksStatus: "failing" }) });
    const open = makeWorktree({ id: "open", pr_info: pr({ state: "OPEN" }) });
    const merged = makeWorktree({ id: "merged", pr_info: pr({ state: "MERGED" }) });
    const noPr = makeWorktree({ id: "no-pr", pr_info: null });
    const sorted = [noPr, merged, open, failing].sort(makeComparator(only("prStatus")));
    expect(sorted.map((w) => w.id)).toEqual(["failing", "open", "merged", "no-pr"]);
  });

  it("orders by last activity with null updated_at sorting last (desc)", () => {
    const recent = makeWorktree({ id: "recent", agent_status: status("idle", "2024-03-01") });
    const older = makeWorktree({ id: "older", agent_status: status("idle", "2024-01-01") });
    const noneStatus = makeWorktree({ id: "none", agent_status: null });
    const sorted = [noneStatus, older, recent].sort(makeComparator(only("lastActivity", "desc")));
    expect(sorted.map((w) => w.id)).toEqual(["recent", "older", "none"]);
  });

  it("orders branches alphabetically", () => {
    const b = makeWorktree({ id: "b", branch: "feature/b" });
    const a = makeWorktree({ id: "a", branch: "feature/a" });
    const sorted = [b, a].sort(makeComparator(only("branchName")));
    expect(sorted.map((w) => w.id)).toEqual(["a", "b"]);
  });

  it("orders dirty worktrees first", () => {
    const dirty = makeWorktree({ id: "dirty", git_status: { ahead: 0, behind: 0, dirty: 3 } });
    const clean = makeWorktree({ id: "clean", git_status: { ahead: 0, behind: 0, dirty: 0 } });
    const sorted = [clean, dirty].sort(makeComparator(only("gitDirty")));
    expect(sorted.map((w) => w.id)).toEqual(["dirty", "clean"]);
  });

  it("sorts projectless worktrees last for linearProject", () => {
    const withProj = makeWorktree({
      id: "with",
      linear_info: { identifier: "ENG-1", title: "t", url: "u", state: { name: "s", color: "c", type: "started" }, priorityLabel: "P", assignee: null, project: { id: "p", name: "Alpha" } },
    });
    const without = makeWorktree({ id: "without" });
    const sorted = [without, withProj].sort(makeComparator(only("linearProject")));
    expect(sorted.map((w) => w.id)).toEqual(["with", "without"]);
  });

  it("negates the comparison for desc direction", () => {
    const a = makeWorktree({ id: "a", branch: "aaa" });
    const b = makeWorktree({ id: "b", branch: "bbb" });
    const asc = [b, a].sort(makeComparator(only("branchName", "asc")));
    const desc = [a, b].sort(makeComparator(only("branchName", "desc")));
    expect(asc.map((w) => w.id)).toEqual(["a", "b"]);
    expect(desc.map((w) => w.id)).toEqual(["b", "a"]);
  });

  it("applies criteria in order, first non-zero wins", () => {
    // isMain (asc) is decisive; branchName never consulted for the main one.
    const main = makeWorktree({ id: "main", is_main: 1, branch: "aaa" });
    const dedicated = makeWorktree({ id: "ded", is_main: 0, branch: "zzz" });
    const criteria: WorktreeSortCriterion[] = [
      { key: "isMain", direction: "asc", enabled: true },
      { key: "branchName", direction: "asc", enabled: true },
    ];
    const sorted = [main, dedicated].sort(makeComparator(criteria));
    expect(sorted.map((w) => w.id)).toEqual(["ded", "main"]);
  });

  it("skips disabled criteria", () => {
    const a = makeWorktree({ id: "a", branch: "aaa", created_at: "2024-01-02" });
    const b = makeWorktree({ id: "b", branch: "bbb", created_at: "2024-01-01" });
    const criteria: WorktreeSortCriterion[] = [
      { key: "branchName", direction: "asc", enabled: false }, // ignored
      { key: "createdAt", direction: "desc", enabled: true },
    ];
    const sorted = [b, a].sort(makeComparator(criteria));
    expect(sorted.map((w) => w.id)).toEqual(["a", "b"]); // newest (a) first
  });

  it("returns 0 (stable) when all criteria tie", () => {
    const a = makeWorktree({ id: "a" });
    const b = makeWorktree({ id: "b" });
    expect(makeComparator(only("isMain"))(a, b)).toBe(0);
  });

  it("default settings reproduce the legacy comparator order", () => {
    const main = makeWorktree({ id: "main", is_main: 1, created_at: "2024-01-05" });
    const eng1a = makeWorktree({ id: "eng1a", linear_info: { identifier: "ENG-1", title: "t", url: "u", state: { name: "s", color: "c", type: "started" }, priorityLabel: "P", assignee: null, project: null }, created_at: "2024-01-01" });
    const eng1b = makeWorktree({ id: "eng1b", linear_info: { identifier: "ENG-1", title: "t", url: "u", state: { name: "s", color: "c", type: "started" }, priorityLabel: "P", assignee: null, project: null }, created_at: "2024-01-03" });
    const plain = makeWorktree({ id: "plain", created_at: "2024-01-04" });
    const legacy = [main, plain, eng1a, eng1b].sort(compareWorktrees);
    const viaDefault = [main, plain, eng1a, eng1b].sort(makeComparator(DEFAULT_SETTINGS.worktreeSort));
    expect(viaDefault.map((w) => w.id)).toEqual(legacy.map((w) => w.id));
    expect(viaDefault.map((w) => w.id)).toEqual(["eng1b", "eng1a", "plain", "main"]);
  });
});

describe("applyWorktreeFilters", () => {
  const noFilters = {
    hideMainBranch: false,
    hideMergedClosedPrs: false,
    hideIdleDoneAgents: false,
    hideWithoutLinearTicket: false,
  };

  it("hides the main/master branch when hideMainBranch is set", () => {
    const main = makeWorktree({ id: "main", is_main: 1, branch: "main" });
    const dedicated = makeWorktree({ id: "ded", is_main: 0, branch: "feature/x" });
    const result = applyWorktreeFilters([main, dedicated], { ...noFilters, hideMainBranch: true });
    expect(result.map((w) => w.id)).toEqual(["ded"]);
  });

  it("keeps the main branch when its session is effectively open", () => {
    const main = makeWorktree({ id: "main", is_main: 1, branch: "main", agent_status: status("executing", "2024-01-01", 1) });
    const result = applyWorktreeFilters([main], { ...noFilters, hideMainBranch: true });
    expect(result.map((w) => w.id)).toEqual(["main"]);
  });

  it("hides merged/closed PRs when hideMergedClosedPrs is set", () => {
    const merged = makeWorktree({ id: "merged", pr_info: pr({ state: "MERGED" }) });
    const closed = makeWorktree({ id: "closed", pr_info: pr({ state: "CLOSED" }) });
    const open = makeWorktree({ id: "open", pr_info: pr({ state: "OPEN" }) });
    const result = applyWorktreeFilters([merged, closed, open], { ...noFilters, hideMergedClosedPrs: true });
    expect(result.map((w) => w.id)).toEqual(["open"]);
  });

  it("hides idle/done/none agents when hideIdleDoneAgents is set", () => {
    const idle = makeWorktree({ id: "idle", agent_status: status("idle") });
    const done = makeWorktree({ id: "done", agent_status: status("done") });
    const none = makeWorktree({ id: "none", agent_status: null });
    const executing = makeWorktree({ id: "exec", agent_status: status("executing") });
    const result = applyWorktreeFilters([idle, done, none, executing], { ...noFilters, hideIdleDoneAgents: true });
    expect(result.map((w) => w.id)).toEqual(["exec"]);
  });

  it("hides worktrees without a Linear ticket when hideWithoutLinearTicket is set", () => {
    const withTicket = makeWorktree({ id: "with", linear_info: { identifier: "ENG-1", title: "t", url: "u", state: { name: "s", color: "c", type: "started" }, priorityLabel: "P", assignee: null, project: null } });
    const without = makeWorktree({ id: "without" });
    const result = applyWorktreeFilters([withTicket, without], { ...noFilters, hideWithoutLinearTicket: true });
    expect(result.map((w) => w.id)).toEqual(["with"]);
  });

  it("returns all worktrees when no filters are set", () => {
    const a = makeWorktree({ id: "a", is_main: 1, branch: "main" });
    const b = makeWorktree({ id: "b" });
    expect(applyWorktreeFilters([a, b], noFilters).map((w) => w.id)).toEqual(["a", "b"]);
  });

  it("default settings enable no filters", () => {
    const main = makeWorktree({ id: "main", is_main: 1, branch: "main" });
    const result = applyWorktreeFilters([main], {
      hideMainBranch: DEFAULT_SETTINGS.hideMainBranch,
      hideMergedClosedPrs: DEFAULT_SETTINGS.hideMergedClosedPrs,
      hideIdleDoneAgents: DEFAULT_SETTINGS.hideIdleDoneAgents,
      hideWithoutLinearTicket: DEFAULT_SETTINGS.hideWithoutLinearTicket,
    });
    // hideMainBranch defaults to true, so the plain main branch is hidden.
    expect(result).toEqual([]);
  });
});
