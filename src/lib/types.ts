export interface Repository {
  id: string;
  path: string;
  name: string;
  last_used_at: string;
}

export interface Worktree {
  id: string;
  repo_id: string;
  path: string;
  branch: string;
  name: string;
  custom_name: string | null;
  nickname_source: string | null;
  is_main: number;
  created_at: string;
}

export interface AgentStatus {
  worktree_id: string;
  status: AgentStatusType;
  last_response: string | null;
  transcript_summary: string | null;
  session_id: string | null;
  is_open: number;
  active_subagents: number;
  updated_at: string;
}

export type AgentStatusType = "none" | "idle" | "executing" | "planning" | "waiting" | "delegating" | "done";

export interface PrInfo {
  number: number;
  title: string;
  url: string;
  state: string;
  isDraft: boolean;
  reviewDecision: string;
  hasFeedback: boolean;
  checksStatus: "pending" | "passing" | "failing" | "none";
  activeCheckUrl: string | null;
  activeCheckName: string | null;
  checksWaiting: boolean;
}

export interface LinearProject {
  id: string;
  name: string;
  color?: string;
  url?: string;
}

export interface LinearInfo {
  identifier: string;
  title: string;
  url: string;
  state: { name: string; color: string; type: string };
  priorityLabel: string;
  assignee: string | null;
  project?: LinearProject | null;
  prAttachment?: {
    url: string;
    title: string;
    metadata: Record<string, unknown>;
  } | null;
}

export interface WorktreeGroup {
  repo: Repository;
  worktrees: WorktreeWithStatus[];
}

export interface WorktreeWithStatus extends Worktree {
  agent_status: AgentStatus | null;
  git_status: GitStatus | null;
  last_commit: CommitInfo | null;
  pr_info: PrInfo | null;
  linear_info: LinearInfo | null;
  has_terminal: boolean;
  open_ide: "cursor" | "vscode" | null;
}

export interface GitStatus {
  ahead: number;
  behind: number;
  dirty: number;
}

export interface CommitInfo {
  hash: string;
  message: string;
  relative_time: string;
}

export type SystemSound =
  | "Basso"
  | "Blow"
  | "Bottle"
  | "Frog"
  | "Funk"
  | "Glass"
  | "Hero"
  | "Morse"
  | "Ping"
  | "Pop"
  | "Purr"
  | "Sosumi"
  | "Submarine"
  | "Tink";

export type AgentPermissionMode =
  | "acceptEdits"
  | "auto"
  | "bypassPermissions"
  | "manual"
  | "dontAsk"
  | "plan";

// Customizable worktree ordering. Each key maps to an ascending comparator in
// the SORT_REGISTRY (src/lib/grouping.ts); direction flips it. The dashboard
// walks the enabled criteria in order and the first non-zero comparison wins.
export type WorktreeSortKey =
  | "isMain" // dedicated worktrees before the main/master branch
  | "repo" // group worktrees by their repository (repo sections ordered by name)
  | "linearTicket" // cluster worktrees sharing a Linear ticket (ticketless last)
  | "linearProject" // by Linear project name (projectless last)
  | "agentStatus" // active agents first (executing > planning > waiting > ...)
  | "lastActivity" // by agent_status.updated_at recency
  | "createdAt" // by worktree creation time
  | "branchName" // alphabetical by branch
  | "prStatus" // PRs needing attention first (failing/changes/pending > ...)
  | "gitDirty"; // worktrees with uncommitted changes first

export interface WorktreeSortCriterion {
  key: WorktreeSortKey;
  direction: "asc" | "desc";
  enabled: boolean;
}

export interface Settings {
  ide: "cursor" | "vscode" | "terminal";
  defaultBranchPrefix: string;
  defaultBaseBranch: string;
  pollingIntervalMs: number;
  autoSyncOnStartup: boolean;
  compactView: boolean;
  hideMainBranch: boolean;
  audioNotifications: boolean;
  audioWaitingSound: SystemSound;
  audioDoneSound: SystemSound;
  ghPrStatus: boolean;
  ghPollingIntervalMs: number;
  logLevel: "debug" | "info" | "warn" | "error";
  linearEnabled: boolean;
  linearUseDesktopApp: boolean;
  linearApiKey: string;
  linearPollingIntervalMs: number;
  ghRefreshOnManual: boolean;
  linearRefreshOnManual: boolean;
  linearAutoNickname: boolean;
  // --- Worktree sorting & display ---
  worktreeSort: WorktreeSortCriterion[];
  // Filters (which worktrees appear). hideMainBranch (above) is also a filter.
  hideMergedClosedPrs: boolean;
  hideIdleDoneAgents: boolean;
  hideWithoutLinearTicket: boolean;
  // Display fields (which per-row info renders). Independent of the gh/linear
  // fetch toggles — you can keep fetching (e.g. to sort) while hiding a badge.
  showPrStatus: boolean;
  showLinearTicket: boolean;
  showGitAheadBehind: boolean;
  showLastCommit: boolean;
  maxLogSizeMb: number;
  agentPermissionMode: AgentPermissionMode;
  agentClaudeArgs: string;
  resumeLastSession: boolean;
  setupCompleted?: boolean;
  lastSeenVersion?: string;
  lastUpdateCheck?: number;
  latestKnownVersion?: string;
}

export type AppMode =
  | "dashboard"
  | "chat"
  | "chat-pick"
  | "new-worktree"
  | "branch-exists"
  | "delete-confirm"
  | "settings"
  | "repo-select"
  | "folder-browse"
  | "creating-worktree"
  | "deleting-worktree"
  | "run-script-prompt"
  | "delete-session-confirm"
  | "setup";

export interface HookEvent {
  event: string;
  session_id?: string;
  cwd?: string;
  hook_event_name?: string;
  permission_mode?: string;
  tool_name?: string;
  tool_input?: Record<string, unknown>;
  tool_response?: string;
  stop_hook_active?: boolean;
  last_assistant_message?: string;
  transcript_summary?: string;
  message?: string;
  title?: string;
  notification_type?: string;
  permission_prompt?: boolean;
}

// A Claude Code session started and driven by am (one per worktree).
// The session id doubles as the claude CLI session UUID (--session-id/--resume).
export interface ManagedSession {
  id: string;
  worktree_id: string;
  cwd: string;
  last_prompt: string | null;
  turn_pid: number | null;
  turn_count: number;
  created_at: string;
  updated_at: string;
}

// One rendered entry of a managed session's transcript.
export interface ChatMessage {
  role: "user" | "assistant" | "tool" | "system" | "error";
  text: string;
  ts?: string;
}

export interface StandaloneSession {
  id: string;
  path: string;
  status: AgentStatusType;
  session_id: string | null;
  last_response: string | null;
  transcript_summary: string | null;
  is_open: number; // 0 or 1
  active_subagents: number;
  created_at: string;
  updated_at: string;
}

