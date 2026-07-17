import React from "react";
import { describe, it, expect, vi } from "vitest";
import { render } from "ink-testing-library";
import { SortEditor, EXAMPLE_WORKTREES } from "../../src/components/SortEditor.js";
import { makeComparator } from "../../src/lib/grouping.js";
import { DEFAULT_SETTINGS } from "../../src/lib/settings.js";
import type { WorktreeSortCriterion, WorktreeSortKey } from "../../src/lib/types.js";

vi.mock("../../src/lib/logger.js", () => ({
  log: vi.fn(),
  initLogger: vi.fn(),
  setLogLevel: vi.fn(),
}));

const ESCAPE = "";
const DOWN = ESCAPE + "[B";
const RIGHT = ESCAPE + "[C";
const ENTER = "\r";

function waitForFrame(ms = 30): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}

// A controlled wrapper so onChange updates re-render the editor, like the parent.
function Harness({ onSaved }: { onSaved: (c: WorktreeSortCriterion[]) => void }) {
  const [criteria, setCriteria] = React.useState<WorktreeSortCriterion[]>(
    DEFAULT_SETTINGS.worktreeSort.map((c) => ({ ...c }))
  );
  React.useEffect(() => {
    onSaved(criteria);
  }, [criteria, onSaved]);
  return <SortEditor criteria={criteria} onChange={setCriteria} onClose={vi.fn()} />;
}

describe("SortEditor", () => {
  it("renders the criteria list and an example preview", () => {
    const { lastFrame } = render(
      <SortEditor criteria={DEFAULT_SETTINGS.worktreeSort} onChange={vi.fn()} onClose={vi.fn()} />
    );
    const frame = lastFrame()!;
    expect(frame).toContain("Edit Sort Order");
    expect(frame).toContain("Dedicated vs main");
    expect(frame).toContain("Example");
    // example branches render in the preview
    expect(frame).toContain("feature/auth");
  });

  it("shows Linear tickets/projects in the example when Linear is on", () => {
    const { lastFrame } = render(
      <SortEditor criteria={DEFAULT_SETTINGS.worktreeSort} onChange={vi.fn()} onClose={vi.fn()} linearEnabled />
    );
    const frame = lastFrame()!;
    expect(frame).toContain("Alpha");
    expect(frame).toMatch(/ENG-\d/);
  });

  it("shows project group headers when Linear project is the top sort key", () => {
    const criteria: WorktreeSortCriterion[] = [
      { key: "linearProject", direction: "asc", enabled: true },
      { key: "linearTicket", direction: "asc", enabled: true },
      ...DEFAULT_SETTINGS.worktreeSort
        .filter((c) => c.key !== "linearProject" && c.key !== "linearTicket")
        .map((c) => ({ ...c, enabled: false })),
    ];
    const { lastFrame } = render(
      <SortEditor criteria={criteria} onChange={vi.fn()} onClose={vi.fn()} linearEnabled />
    );
    const frame = lastFrame()!;
    expect(frame).toContain("═ Alpha ═");
    expect(frame).toContain("═ Beta ═");
    expect(frame).toContain("═ Gamma ═");
  });

  it("shows repo group headers when Repository is the top sort key", () => {
    const criteria: WorktreeSortCriterion[] = [
      { key: "repo", direction: "asc", enabled: true },
      ...DEFAULT_SETTINGS.worktreeSort
        .filter((c) => c.key !== "repo")
        .map((c) => ({ ...c })),
    ];
    const { lastFrame } = render(
      <SortEditor criteria={criteria} onChange={vi.fn()} onClose={vi.fn()} linearEnabled />
    );
    const frame = lastFrame()!;
    expect(frame).toContain("── agent-monitor ───");
    expect(frame).toContain("── web-app ───");
  });

  it("example spans multiple repositories", () => {
    const repos = new Set(EXAMPLE_WORKTREES.map((w) => w.repo_id));
    expect(repos.size).toBeGreaterThanOrEqual(2);
  });

  it("does not show project headers when Linear project is not the top key", () => {
    // Default sort leads with isMain, so no project grouping headers appear.
    const { lastFrame } = render(
      <SortEditor criteria={DEFAULT_SETTINGS.worktreeSort} onChange={vi.fn()} onClose={vi.fn()} linearEnabled />
    );
    expect(lastFrame()!).not.toContain("═ Alpha ═");
  });

  it("omits Linear tickets/projects from the example when Linear is off", () => {
    const { lastFrame } = render(
      <SortEditor criteria={DEFAULT_SETTINGS.worktreeSort} onChange={vi.fn()} onClose={vi.fn()} linearEnabled={false} />
    );
    const frame = lastFrame()!;
    // branches still render, but no ticket identifiers or project names
    expect(frame).toContain("feature/auth");
    expect(frame).not.toMatch(/ENG-\d/);
    expect(frame).not.toContain("Alpha");
  });

  it("shows a distinct hint when an item is grabbed", async () => {
    const { stdin, lastFrame } = render(
      <SortEditor criteria={DEFAULT_SETTINGS.worktreeSort} onChange={vi.fn()} onClose={vi.fn()} />
    );
    await waitForFrame();
    expect(lastFrame()!).toContain("Grab to move");
    stdin.write(ENTER); // grab
    await waitForFrame();
    expect(lastFrame()!).toContain("Drop");
  });

  it("reorders via grab, move, drop", async () => {
    let latest: WorktreeSortCriterion[] = [];
    const { stdin } = render(<Harness onSaved={(c) => (latest = c)} />);
    await waitForFrame();
    stdin.write(ENTER); // grab isMain (cursor at 0)
    await waitForFrame();
    stdin.write(DOWN); // move it down past linearTicket
    await waitForFrame();
    stdin.write(ENTER); // drop
    await waitForFrame();
    expect(latest[0].key).toBe("linearTicket");
    expect(latest[1].key).toBe("isMain");
  });

  it("toggles enabled with Space", async () => {
    let latest: WorktreeSortCriterion[] = [];
    const { stdin } = render(<Harness onSaved={(c) => (latest = c)} />);
    await waitForFrame();
    const before = latest[0].enabled;
    stdin.write(" ");
    await waitForFrame();
    expect(latest[0].enabled).toBe(!before);
  });

  it("flips direction with the right arrow", async () => {
    let latest: WorktreeSortCriterion[] = [];
    const { stdin } = render(<Harness onSaved={(c) => (latest = c)} />);
    await waitForFrame();
    const before = latest[0].direction;
    stdin.write(RIGHT);
    await waitForFrame();
    expect(latest[0].direction).not.toBe(before);
  });

  it("example data exercises every sort dimension distinctly", () => {
    const wts = EXAMPLE_WORKTREES;

    // isMain: exactly one main worktree, at least one dedicated.
    expect(wts.filter((w) => w.is_main === 1)).toHaveLength(1);
    expect(wts.some((w) => w.is_main === 0)).toBe(true);

    // linearTicket: several distinct tickets + at least one ticketless.
    const tickets = wts.map((w) => w.linear_info?.identifier).filter(Boolean);
    expect(new Set(tickets).size).toBeGreaterThanOrEqual(4);
    expect(wts.some((w) => !w.linear_info)).toBe(true);

    // linearProject: at least three distinct projects + at least one projectless.
    const projects = wts.map((w) => w.linear_info?.project?.name).filter(Boolean);
    expect(new Set(projects).size).toBeGreaterThanOrEqual(3);
    expect(wts.some((w) => !w.linear_info?.project)).toBe(true);

    // agentStatus: all statuses represented (none via a null agent_status).
    const statuses = new Set(wts.map((w) => w.agent_status?.status ?? "none"));
    for (const s of ["executing", "planning", "waiting", "idle", "done", "none"]) {
      expect(statuses.has(s as any)).toBe(true);
    }

    // lastActivity & createdAt: distinct timestamps so ordering is unambiguous.
    const activities = wts.map((w) => w.agent_status?.updated_at).filter(Boolean);
    expect(new Set(activities).size).toBe(activities.length);
    expect(new Set(wts.map((w) => w.created_at)).size).toBe(wts.length);

    // branchName: distinct.
    expect(new Set(wts.map((w) => w.branch)).size).toBe(wts.length);

    // gitDirty: some dirty, some clean.
    expect(wts.some((w) => (w.git_status?.dirty ?? 0) > 0)).toBe(true);
    expect(wts.some((w) => (w.git_status?.dirty ?? 0) === 0)).toBe(true);

    // prStatus: a spread of PR states/checks + at least one without a PR.
    const prStates = new Set(
      wts.filter((w) => w.pr_info).map((w) => {
        const p = w.pr_info!;
        if (p.checksStatus === "failing") return "failing";
        if (p.hasFeedback || p.reviewDecision === "CHANGES_REQUESTED") return "changes";
        if (p.checksStatus === "pending") return "pending";
        if (p.isDraft) return "draft";
        if (p.state === "MERGED") return "merged";
        if (p.state === "CLOSED") return "closed";
        return "open";
      })
    );
    expect(prStates.size).toBeGreaterThanOrEqual(5);
    expect(wts.some((w) => !w.pr_info)).toBe(true);

    // Every criterion produces an order distinct from the default sort, so its
    // effect is visible when the user enables it.
    const keys: WorktreeSortKey[] = [
      "linearProject", "agentStatus", "lastActivity", "branchName", "prStatus", "gitDirty",
    ];
    const defaultOrder = [...wts]
      .sort(makeComparator(DEFAULT_SETTINGS.worktreeSort))
      .map((w) => w.id)
      .join(",");
    for (const key of keys) {
      const sorted = [...wts]
        .sort(makeComparator([{ key, direction: "asc", enabled: true }]))
        .map((w) => w.id)
        .join(",");
      expect(sorted, `${key} should reorder the example set`).not.toBe(defaultOrder);
    }
  });

  it("calls onClose on Esc when nothing is grabbed", async () => {
    const onClose = vi.fn();
    const { stdin } = render(
      <SortEditor criteria={DEFAULT_SETTINGS.worktreeSort} onChange={vi.fn()} onClose={onClose} />
    );
    await waitForFrame();
    stdin.write(ESCAPE);
    await waitForFrame();
    expect(onClose).toHaveBeenCalled();
  });
});
