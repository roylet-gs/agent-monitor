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
  created_at: string;
}

export interface AgentStatus {
  worktree_id: string;
  status: AgentStatusType;
  plan_mode: number; // 0 or 1 (sqlite boolean)
  last_response: string | null;
  session_id: string | null;
  updated_at: string;
}

export type AgentStatusType =
  | "idle"
  | "thinking"
  | "planning"
  | "executing"
  | "error"
  | "waiting_for_input"
  | "completed";

export interface WorktreeWithStatus extends Worktree {
  agent_status: AgentStatus | null;
  git_status: GitStatus | null;
  last_commit: CommitInfo | null;
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
  pollingIntervalMs: number;
  autoInstallHooks: boolean;
  autoSyncOnStartup: boolean;
  logLevel: "debug" | "info" | "warn" | "error";
}

export type AppMode =
  | "dashboard"
  | "new-worktree"
  | "branch-exists"
  | "delete-confirm"
  | "settings"
  | "repo-select"
  | "folder-browse";

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
