import type { AgentStatus, AgentStatusType, StandaloneSession } from "./types.js";

const STALE_THRESHOLD_MS = 5 * 60 * 1000; // 5 minutes

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
  // Safety net: if executing/planning with no event for 5+ minutes, the session
  // is likely stuck (e.g. Claude crashed without firing Stop). Display as
  // "waiting" so the user knows it needs attention. This is display-time only —
  // the DB status is preserved and resumes correctly when events arrive.
  if (
    (agentStatus.status === "executing" || agentStatus.status === "planning") &&
    Date.now() - new Date(agentStatus.updated_at + "Z").getTime() > STALE_THRESHOLD_MS
  ) {
    return "waiting";
  }
  return agentStatus.status;
}

export function getDisplayStatusStandalone(session: StandaloneSession): AgentStatusType {
  if (
    (session.status === "executing" || session.status === "planning") &&
    Date.now() - new Date(session.updated_at + "Z").getTime() > STALE_THRESHOLD_MS
  ) {
    return "waiting";
  }
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

// Flattens a markdown-ish transcript summary into a single line of plain text
// suitable for Ink's truncate-end rendering. Strips common markdown syntax
// (headings, bold/italic/code, list/blockquote markers) and collapses all
// whitespace (including newlines) into single spaces.
export function normalizeSummary(text: string, maxChars = 200): string {
  return text
    .replace(/^\s*#{1,6}\s+/gm, "")
    .replace(/^\s*>\s+/gm, "")
    .replace(/^\s*[-*+]\s+/gm, "")
    .replace(/^\s*\d+\.\s+/gm, "")
    .replace(/\*\*(.+?)\*\*/g, "$1")
    .replace(/__(.+?)__/g, "$1")
    .replace(/`([^`]+)`/g, "$1")
    .replace(/\s+/g, " ")
    .trim()
    .slice(0, maxChars);
}
