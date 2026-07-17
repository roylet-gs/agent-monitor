import React, { useState } from "react";
import { Box, Text, useInput } from "ink";
import { makeComparator } from "../lib/grouping.js";
import type { AgentStatus, AgentStatusType, WorktreeSortCriterion, WorktreeSortKey, WorktreeWithStatus } from "../lib/types.js";

export const SORT_KEY_LABELS: Record<WorktreeSortKey, string> = {
  isMain: "Dedicated vs main",
  repo: "Repository",
  linearTicket: "Linear ticket",
  linearProject: "Linear project",
  agentStatus: "Agent status",
  lastActivity: "Last activity",
  createdAt: "Created date",
  branchName: "Branch name",
  prStatus: "PR status",
  gitDirty: "Uncommitted changes",
};

// A short human hint of what a criterion sorts by, shown per row.
const SORT_KEY_HINTS: Record<WorktreeSortKey, string> = {
  isMain: "dedicated worktrees first, main/master last",
  repo: "group worktrees by repository (sections ordered by name)",
  linearTicket: "cluster worktrees sharing a ticket",
  linearProject: "group by Linear project name",
  agentStatus: "active agents first (executing → idle → none)",
  lastActivity: "most recently active first",
  createdAt: "newest / oldest worktrees first",
  branchName: "alphabetical by branch",
  prStatus: "PRs needing attention first",
  gitDirty: "worktrees with uncommitted changes first",
};

interface SortEditorProps {
  criteria: WorktreeSortCriterion[];
  onChange: (next: WorktreeSortCriterion[]) => void;
  onClose: () => void;
  /** When false, the example omits Linear tickets/projects (integration off). */
  linearEnabled?: boolean;
}

// ---- Example worktrees for the live preview --------------------------------
// A curated set chosen so every sort dimension visibly changes the order.
function ex(overrides: Partial<WorktreeWithStatus>): WorktreeWithStatus {
  return {
    id: overrides.id ?? "x",
    repo_id: "demo",
    path: "/demo",
    branch: "branch",
    name: "branch",
    custom_name: null,
    nickname_source: null,
    is_main: 0,
    created_at: "2024-01-01T00:00:00.000Z",
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

function agent(status: AgentStatusType, updated_at: string): AgentStatus {
  return { worktree_id: "x", status, last_response: null, transcript_summary: null, session_id: null, is_open: 0, updated_at, active_subagents: 0 };
}

function linear(identifier: string, projectName: string): NonNullable<WorktreeWithStatus["linear_info"]> {
  return {
    identifier,
    title: identifier,
    url: "",
    state: { name: "In Progress", color: "#0ea5e9", type: "started" },
    priorityLabel: "High",
    assignee: null,
    project: { id: `p-${projectName}`, name: projectName },
  };
}

function pr(number: number, over: Partial<NonNullable<WorktreeWithStatus["pr_info"]>>): NonNullable<WorktreeWithStatus["pr_info"]> {
  return {
    number,
    title: `PR #${number}`,
    url: "",
    state: "OPEN",
    isDraft: false,
    reviewDecision: "",
    hasFeedback: false,
    checksStatus: "none",
    activeCheckUrl: null,
    activeCheckName: null,
    checksWaiting: false,
    ...over,
  };
}

// Curated so that every sort criterion visibly changes the order:
//  - isMain: one main worktree (last under the default sort)
//  - linearTicket: five distinct tickets + two ticketless (cluster/last)
//  - linearProject: three projects (Alpha×3, Beta, Gamma) + two projectless
//  - agentStatus: executing / planning / waiting / idle / done / none
//  - lastActivity & createdAt: every row has a distinct timestamp
//  - branchName: distinct branches
//  - prStatus: failing / changes / pending / draft / merged / closed / none
//  - gitDirty: three dirty, the rest clean
//  - repo: worktrees split across two repositories (repo_id doubles as name)
export const EXAMPLE_WORKTREES: WorktreeWithStatus[] = [
  ex({
    id: "e-auth",
    repo_id: "web-app",
    branch: "feature/auth",
    created_at: "2024-03-10T09:00:00.000Z",
    agent_status: agent("executing", "2024-03-12T15:30:00.000Z"),
    git_status: { ahead: 2, behind: 0, dirty: 4 },
    pr_info: pr(21, { checksStatus: "pending" }),
    linear_info: linear("ENG-2", "Alpha"),
  }),
  ex({
    id: "e-hotfix",
    repo_id: "agent-monitor",
    branch: "hotfix/prod",
    created_at: "2024-03-09T09:00:00.000Z",
    agent_status: agent("planning", "2024-03-12T10:30:00.000Z"),
    git_status: { ahead: 1, behind: 0, dirty: 1 },
    pr_info: pr(25, { checksStatus: "failing" }),
    linear_info: linear("ENG-3", "Alpha"),
  }),
  ex({
    id: "e-bug",
    repo_id: "web-app",
    branch: "fix/login-crash",
    created_at: "2024-03-08T09:00:00.000Z",
    agent_status: agent("waiting", "2024-03-11T15:30:00.000Z"),
    git_status: { ahead: 0, behind: 1, dirty: 0 },
    pr_info: pr(22, { reviewDecision: "CHANGES_REQUESTED", hasFeedback: true }),
    linear_info: linear("ENG-1", "Alpha"),
  }),
  ex({
    id: "e-search",
    repo_id: "web-app",
    branch: "feature/search",
    created_at: "2024-03-07T09:00:00.000Z",
    agent_status: agent("idle", "2024-03-10T15:30:00.000Z"),
    git_status: { ahead: 0, behind: 0, dirty: 2 },
    pr_info: pr(30, { isDraft: true }),
    linear_info: linear("GAM-1", "Gamma"),
  }),
  ex({
    id: "e-api",
    repo_id: "agent-monitor",
    branch: "feature/api",
    created_at: "2024-03-05T09:00:00.000Z",
    agent_status: agent("done", "2024-03-09T15:30:00.000Z"),
    git_status: { ahead: 0, behind: 0, dirty: 0 },
    pr_info: pr(12, { state: "MERGED", reviewDecision: "APPROVED", checksStatus: "passing" }),
    linear_info: linear("BUG-9", "Beta"),
  }),
  ex({
    id: "e-chore",
    repo_id: "agent-monitor",
    branch: "chore/cleanup",
    created_at: "2024-03-02T09:00:00.000Z",
    agent_status: null,
    git_status: { ahead: 0, behind: 0, dirty: 0 },
    pr_info: pr(18, { state: "CLOSED" }),
  }),
  ex({
    id: "e-main",
    repo_id: "agent-monitor",
    branch: "main",
    is_main: 1,
    created_at: "2024-01-15T09:00:00.000Z",
    agent_status: null,
  }),
];

function statusColor(status: string | undefined): string {
  switch (status) {
    case "executing": return "green";
    case "planning": return "cyan";
    case "waiting": return "yellow";
    case "done": return "blueBright";
    default: return "gray";
  }
}

function prLabel(wt: WorktreeWithStatus): { text: string; color: string } | null {
  const pr = wt.pr_info;
  if (!pr) return null;
  if (pr.checksStatus === "failing") return { text: `#${pr.number}✗`, color: "red" };
  if (pr.hasFeedback || pr.reviewDecision === "CHANGES_REQUESTED") return { text: `#${pr.number}⟳`, color: "yellow" };
  if (pr.checksStatus === "pending") return { text: `#${pr.number}◌`, color: "cyan" };
  if (pr.state === "OPEN" && pr.isDraft) return { text: `#${pr.number}✎draft`, color: "gray" };
  if (pr.state === "MERGED") return { text: `#${pr.number}⤵`, color: "magenta" };
  if (pr.state === "CLOSED") return { text: `#${pr.number}✕`, color: "gray" };
  return { text: `#${pr.number}○`, color: "green" };
}

// A friendly relative-time label from an ISO timestamp, relative to the newest
// example activity so the preview reads sensibly without a real clock.
const NEWEST_TS = Date.parse("2024-03-12T15:30:00.000Z");
function activityLabel(wt: WorktreeWithStatus): string {
  const ts = wt.agent_status?.updated_at;
  if (!ts) return "—";
  const diffH = Math.round((NEWEST_TS - Date.parse(ts)) / 3_600_000);
  if (diffH <= 0) return "just now";
  if (diffH < 24) return `${diffH}h ago`;
  return `${Math.round(diffH / 24)}d ago`;
}

export function SortEditor({ criteria, onChange, onClose, linearEnabled = true }: SortEditorProps) {
  const [cursor, setCursor] = useState(0);
  const [grabbed, setGrabbed] = useState(false);

  // Without Linear integration there are no tickets/projects to show or sort by.
  const examples = linearEnabled
    ? EXAMPLE_WORKTREES
    : EXAMPLE_WORKTREES.map((w) => ({ ...w, linear_info: null }));

  const move = (from: number, to: number) => {
    if (to < 0 || to >= criteria.length) return;
    const next = [...criteria];
    [next[from], next[to]] = [next[to], next[from]];
    onChange(next);
    setCursor(to);
  };

  useInput((input, key) => {
    if (key.escape) {
      if (grabbed) setGrabbed(false);
      else onClose();
      return;
    }
    if (key.return) {
      setGrabbed((g) => !g);
      return;
    }
    if (key.upArrow) {
      if (grabbed) move(cursor, cursor - 1);
      else setCursor((c) => Math.max(0, c - 1));
      return;
    }
    if (key.downArrow) {
      if (grabbed) move(cursor, cursor + 1);
      else setCursor((c) => Math.min(criteria.length - 1, c + 1));
      return;
    }
    // Enable/disable and direction only when not carrying an item.
    if (!grabbed && input === " ") {
      onChange(criteria.map((c, i) => (i === cursor ? { ...c, enabled: !c.enabled } : c)));
      return;
    }
    if (!grabbed && (key.leftArrow || key.rightArrow)) {
      onChange(
        criteria.map((c, i) =>
          i === cursor ? { ...c, direction: c.direction === "asc" ? ("desc" as const) : ("asc" as const) } : c
        )
      );
      return;
    }
  });

  const activeCount = criteria.filter((c) => c.enabled).length;
  // The top enabled criterion decides the visible grouping in the preview:
  // "repo" → repo section headers, "linearProject" → project cluster headers.
  const firstEnabled = criteria.find((c) => c.enabled);
  const topKey = firstEnabled?.key;
  const groupByRepo = topKey === "repo";
  const showProjectHeaders = linearEnabled && topKey === "linearProject";

  // Repo is a no-op in makeComparator (structural), so when it is the top key
  // sort by repo name (honoring direction) before applying the rest.
  const cmp = makeComparator(criteria);
  const previewSorted = [...examples].sort((a, b) => {
    if (groupByRepo) {
      const r = a.repo_id.localeCompare(b.repo_id);
      if (r !== 0) return firstEnabled!.direction === "desc" ? -r : r;
    }
    return cmp(a, b);
  });

  return (
    <Box flexDirection="column" borderStyle="single" paddingX={1}>
      <Text bold color="cyan">Edit Sort Order</Text>
      <Text dimColor>
        Worktrees are ordered by the enabled criteria top-to-bottom; the first one that differs decides.
      </Text>

      <Box marginTop={1}>
        {/* ---- Criteria list ---- */}
        <Box flexDirection="column" width={40}>
          <Text bold dimColor>Criteria (priority order)</Text>
          {criteria.map((c, i) => {
            const atCursor = i === cursor;
            const carrying = atCursor && grabbed;
            const marker = carrying ? "✥" : atCursor ? "▸" : " ";
            const rank = c.enabled ? `${criteria.slice(0, i + 1).filter((x) => x.enabled).length}.` : "  ";
            return (
              <Text
                key={c.key}
                color={carrying ? "yellow" : atCursor ? "cyan" : undefined}
                dimColor={!c.enabled && !atCursor}
              >
                {marker} <Text dimColor>{rank}</Text>{" "}
                <Text color={c.enabled ? "green" : "gray"}>[{c.enabled ? "✓" : " "}]</Text>{" "}
                {SORT_KEY_LABELS[c.key].padEnd(20)}
                {" "}
                {c.enabled ? (c.direction === "asc" ? "↑ asc" : "↓ desc") : "     "}
              </Text>
            );
          })}
          <Box marginTop={1}>
            <Text dimColor>{activeCount} of {criteria.length} criteria enabled</Text>
          </Box>
        </Box>

        {/* ---- Live example preview ---- */}
        <Box flexDirection="column" marginLeft={2} flexGrow={1}>
          <Text bold dimColor>Example — worktrees in this order</Text>
          {(() => {
            const rows: React.ReactNode[] = [];
            let prevRepo: string | null = null;
            let prevProject: string | null = null;
            let first = true;
            for (const wt of previewSorted) {
              if (groupByRepo && wt.repo_id !== prevRepo) {
                rows.push(
                  <Box key={`rh-${wt.repo_id}`} marginTop={first ? 0 : 1}>
                    <Text dimColor>── {wt.repo_id} ───</Text>
                  </Box>
                );
                prevProject = null; // reset project run inside a new repo
              }
              prevRepo = wt.repo_id;

              const projectName = wt.linear_info?.project?.name ?? null;
              if (showProjectHeaders && projectName && projectName !== prevProject) {
                rows.push(
                  <Text key={`ph-${projectName}`} bold color="magentaBright">═ {projectName} ═</Text>
                );
              }
              prevProject = projectName;
              first = false;

              const pr = prLabel(wt);
              const dirty = (wt.git_status?.dirty ?? 0) > 0;
              const st = wt.agent_status?.status;
              const indent = (showProjectHeaders && projectName) || groupByRepo ? 1 : 0;
              rows.push(
                <Box key={wt.id} gap={1} paddingLeft={indent}>
                  <Text color={statusColor(st)}>{st ? (st === "done" ? "✓" : "●") : "○"}</Text>
                  <Text>{wt.branch.padEnd(15)}</Text>
                  {wt.is_main === 1 ? (
                    <Text dimColor>[branch]</Text>
                  ) : wt.linear_info ? (
                    <Text color="magentaBright">
                      {wt.linear_info.identifier}
                      {/* Project shown in the header when grouped, else inline */}
                      {!showProjectHeaders && wt.linear_info.project ? `·${wt.linear_info.project.name}` : ""}
                    </Text>
                  ) : null}
                  {pr && <Text color={pr.color}>{pr.text}</Text>}
                  {dirty && <Text color="yellow">✎</Text>}
                  <Text dimColor>{activityLabel(wt)}</Text>
                </Box>
              );
            }
            return rows;
          })()}
          <Text dimColor> </Text>
          <Text dimColor>
            {groupByRepo
              ? "Sorting by Repository first groups worktrees under each repo."
              : showProjectHeaders
              ? "Sorting by Linear project first groups worktrees under each project."
              : "Updates live as you change the criteria on the left."}
          </Text>
        </Box>
      </Box>

      {/* ---- Full-width hint for the selected criterion ---- */}
      <Box marginTop={1}>
        <Text>
          <Text bold color="cyan">{SORT_KEY_LABELS[criteria[cursor].key]}</Text>
          <Text dimColor> — {SORT_KEY_HINTS[criteria[cursor].key]}</Text>
        </Text>
      </Box>

      {/* ---- Footer hints ---- */}
      <Box
        marginTop={1}
        borderStyle="single"
        borderTop
        borderBottom={false}
        borderLeft={false}
        borderRight={false}
      >
        <Text>
          {grabbed ? (
            <>
              <Text color="yellow">[↑↓]</Text> Move item{" "}
              <Text color="yellow">[Enter]</Text> Drop{" "}
              <Text color="yellow">[Esc]</Text> Drop
            </>
          ) : (
            <>
              <Text color="yellow">[↑↓]</Text> Select{" "}
              <Text color="yellow">[Enter]</Text> Grab to move{" "}
              <Text color="yellow">[Space]</Text> On/Off{" "}
              <Text color="yellow">[←→]</Text> Direction{" "}
              <Text color="yellow">[Esc]</Text> Back to Settings
            </>
          )}
        </Text>
      </Box>
    </Box>
  );
}
