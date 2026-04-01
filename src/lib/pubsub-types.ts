import type { AgentStatusType, PendingInput } from "./types.js";

export interface AgentStatusUpdateMessage {
  type: "agent-status-update";
  worktreeId: string;
  status: AgentStatusType;
  sessionId: string | null;
  lastResponse: string | null;
  transcriptSummary: string | null;
  isOpen: boolean;
  updatedAt: string;
}

export interface GitActivityMessage {
  type: "git-activity";
  worktreeId: string;
  repoId: string;
  branch: string;
  activity: "push" | "pr-create";
}

export interface StandaloneStatusUpdateMessage {
  type: "standalone-status-update";
  sessionPath: string;
  status: AgentStatusType;
  sessionId: string | null;
  lastResponse: string | null;
  transcriptSummary: string | null;
  isOpen: boolean;
  updatedAt: string;
}

export interface PendingInputMessage {
  type: "pending-input";
  input: PendingInput;
  hookConnectionId: string;
}

export type PubSubMessage = AgentStatusUpdateMessage | GitActivityMessage | StandaloneStatusUpdateMessage | PendingInputMessage;
