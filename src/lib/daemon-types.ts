import type { WorktreeGroup, WorktreeWithStatus, StandaloneSession, PendingInput } from "./types.js";
import type { PubSubMessage } from "./pubsub-types.js";

// --- hook-event → Daemon (existing messages, unchanged) ---
// AgentStatusUpdateMessage | GitActivityMessage | StandaloneStatusUpdateMessage
// These are already defined in pubsub-types.ts

// --- TUI → Daemon (new) ---

export interface SubscribeMessage {
  type: "subscribe";
}

export interface ForceRefreshMessage {
  type: "force-refresh";
  id: string;
  includeIntegrations: boolean;
}

export interface ConfigReloadMessage {
  type: "config-reload";
}

export interface SendResponseMessage {
  type: "send-response";
  inputId: string;
  response: string;
  decision?: "allow" | "deny";
}

export interface SendPromptMessage {
  type: "send-prompt";
  worktreeId: string;
  message: string;
}

export type TuiToDaemonMessage = SubscribeMessage | ForceRefreshMessage | ConfigReloadMessage | SendResponseMessage | SendPromptMessage;

// --- Daemon → TUI (new) ---

export interface DaemonData {
  groups: WorktreeGroup[];
  flatWorktrees: WorktreeWithStatus[];
  standaloneSessions: StandaloneSession[];
}

export interface RefreshResultMessage {
  type: "refresh-result";
  id: string | null;
  data: DaemonData;
}

export interface AgentUpdatePassthroughMessage {
  type: "agent-update";
  original: PubSubMessage;
}

export interface PendingInputNotification {
  type: "pending-input-notify";
  input: PendingInput;
}

export interface PromptSentNotification {
  type: "prompt-sent";
  worktreeId: string;
  success: boolean;
  error?: string;
}

export interface PendingInputResolvedNotification {
  type: "pending-input-resolved";
  inputId: string;
}

export type DaemonToTuiMessage = RefreshResultMessage | AgentUpdatePassthroughMessage | PendingInputNotification | PromptSentNotification | PendingInputResolvedNotification;

// --- All messages the daemon socket can receive ---
export type DaemonInboundMessage = PubSubMessage | TuiToDaemonMessage;
