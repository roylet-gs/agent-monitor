import type { AgentStatusType } from "./types.js";

export interface AgentStatusUpdateMessage {
  type: "agent-status-update";
  worktreeId: string;
  status: AgentStatusType;
  sessionId: string | null;
  lastResponse: string | null;
  transcriptSummary: string | null;
  updatedAt: string;
}

export type PubSubMessage = AgentStatusUpdateMessage;
