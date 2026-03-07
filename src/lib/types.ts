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
  created_at: string;
}

export interface AgentStatus {
  worktree_id: string;
  status: AgentStatusType;
  last_response: string | null;
  transcript_summary: string | null;
  session_id: string | null;
  is_open: number;
  updated_at: string;
}

export type AgentStatusType = "idle" | "executing" | "planning" | "waiting" | "done";

export interface PrInfo {
  number: number;
  title: string;
  url: string;
  state: string;
  isDraft: boolean;
  reviewDecision: string;
  hasFeedback: boolean;
  checksStatus: "pending" | "passing" | "failing" | "none";
}

export interface LinearInfo {
  identifier: string;
  title: string;
  url: string;
  state: { name: string; color: string; type: string };
  priorityLabel: string;
  assignee: string | null;
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

export interface Settings {
  ide: "cursor" | "vscode" | "terminal";
  defaultBranchPrefix: string;
  defaultBaseBranch: string;
  pollingIntervalMs: number;
  autoSyncOnStartup: boolean;
  compactView: boolean;
  hideMainBranch: boolean;
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
  maxLogSizeMb: number;
  applyGlobalRulesEnabled: boolean;
  setupCompleted?: boolean;
  lastSeenVersion?: string;
  lastUpdateCheck?: number;
  latestKnownVersion?: string;
}

export type AppMode =
  | "dashboard"
  | "new-worktree"
  | "branch-exists"
  | "delete-confirm"
  | "settings"
  | "repo-select"
  | "folder-browse"
  | "creating-worktree"
  | "deleting-worktree"
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

export interface Rule {
  id: string;
  tool: string;
  input_pattern?: string;
  decision: "allow" | "deny";
  source: "manual" | "learned";
  created_at: string;
}
