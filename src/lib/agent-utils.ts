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
    (agentStatus.status === "executing" || agentStatus.status === "planning" || agentStatus.status === "delegating") &&
    Date.now() - new Date(agentStatus.updated_at + "Z").getTime() > STALE_THRESHOLD_MS
  ) {
    return "waiting";
  }
  return agentStatus.status;
}

export function getDisplayStatusStandalone(session: StandaloneSession): AgentStatusType {
  if (
    (session.status === "executing" || session.status === "planning" || session.status === "delegating") &&
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
// (headings, bold/italic/code, list/blockquote markers, links) and emoji, then
// collapses all whitespace (including newlines) into single spaces.
//
// Two of these steps exist specifically to keep the detail panel's single-line
// render from overflowing its border (which wraps the bordered box and breaks
// the vertical panel separators):
//   - Links are reduced to their label so long URLs don't survive as content.
//   - Emoji are removed because Ink's truncation (cli-truncate) miscounts their
//     display width: each emoji retained in a truncated line makes the rendered
//     line one column too wide, so it wraps in the terminal and breaks the
//     borders. Width-1 glyphs (✓, ✗, →) and CJK are left intact — those
//     truncate correctly.
// A pictographic base optionally followed by variation selectors / skin-tone
// modifiers, and any number of ZWJ-joined pictographics (e.g. 👨‍👩‍👧). ️ =
// variation selector-16, ‍ = zero-width joiner.
const EMOJI_MOD = "(?:\\uFE0F|[\\u{1F3FB}-\\u{1F3FF}])*";
const EMOJI = new RegExp(`\\p{Extended_Pictographic}${EMOJI_MOD}(?:\\u200D\\p{Extended_Pictographic}${EMOJI_MOD})*`, "gu");
// Regional-indicator letters that compose flag emoji (🇬🇧).
const REGIONAL_INDICATOR = /[\u{1F1E6}-\u{1F1FF}]/gu;

export function normalizeSummary(text: string, maxChars = 200): string {
  return text
    .replace(/^\s*#{1,6}\s+/gm, "")
    .replace(/^\s*>\s+/gm, "")
    .replace(/^\s*[-*+]\s+/gm, "")
    .replace(/^\s*\d+\.\s+/gm, "")
    .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
    .replace(/\*\*(.+?)\*\*/g, "$1")
    .replace(/__(.+?)__/g, "$1")
    .replace(/`([^`]+)`/g, "$1")
    .replace(EMOJI, "")
    .replace(REGIONAL_INDICATOR, "")
    .replace(/\s+/g, " ")
    .trim()
    .slice(0, maxChars);
}
