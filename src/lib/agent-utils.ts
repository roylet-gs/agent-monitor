import type { AgentStatus, AgentStatusType, StandaloneSession } from "./types.js";

export function isEffectivelyOpen(agentStatus: AgentStatus | null | undefined): boolean {
  if (!agentStatus?.is_open) return false;
  if (agentStatus.status === "idle") {
    const updatedAt = new Date(agentStatus.updated_at + "Z").getTime();
    if (updatedAt < Date.now() - 10 * 60 * 1000) return false;
  }
  return true;
}

export function getDisplayStatus(agentStatus: AgentStatus | null | undefined): AgentStatusType {
  if (!agentStatus) return "none";
  return agentStatus.status;
}

export function getDisplayStatusStandalone(session: StandaloneSession): AgentStatusType {
  return session.status;
}

export function isEffectivelyOpenStandalone(session: StandaloneSession): boolean {
  if (session.is_open) return true;
  if (session.status === "idle") {
    const updatedAt = new Date(session.updated_at + "Z").getTime();
    if (updatedAt < Date.now() - 10 * 60 * 1000) return false;
  }
  // Show recently-closed sessions for a grace period
  const updatedAt = new Date(session.updated_at + "Z").getTime();
  return updatedAt > Date.now() - 10 * 60 * 1000;
}
