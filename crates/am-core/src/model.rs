//! Shared data model for the dashboard. Field names of the serde types match
//! the TS JSON shapes (`src/lib/types.ts`) because `integration_cache.payload`
//! is read and written by both apps during the migration.

use serde::{Deserialize, Serialize};

use crate::harness::HarnessKind;
use crate::status::Status;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repository {
    pub id: String,
    pub path: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub id: String,
    pub repo_id: String,
    pub path: String,
    pub branch: String,
    pub name: String,
    pub custom_name: Option<String>,
    pub is_main: bool,
    /// SQLite `datetime('now')` text, e.g. `2026-10-11 09:12:57`.
    pub created_at: String,
}

/// Synthetic repo id for the "Other sessions" group (agent sessions running
/// outside any tracked worktree).
pub const STANDALONE_REPO_ID: &str = "__standalone__";

impl Worktree {
    pub fn is_standalone(&self) -> bool {
        self.repo_id == STANDALONE_REPO_ID
    }

    pub fn display_name(&self) -> &str {
        self.custom_name.as_deref().filter(|n| !n.is_empty()).unwrap_or(&self.branch)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentState {
    pub status: Status,
    pub session_id: Option<String>,
    pub last_response: Option<String>,
    pub is_open: bool,
    pub active_subagents: i64,
    pub updated_at: String,
    /// The agent CLI the session belongs to.
    pub agent: HarnessKind,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitStatus {
    pub ahead: u32,
    pub behind: u32,
    pub dirty: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitInfo {
    pub hash: String,
    pub message: String,
    /// Unix seconds; relative time is computed at draw time (a pre-rendered
    /// "3 minutes ago" string was a source of spurious re-renders in the TS app).
    pub timestamp: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChecksStatus {
    Pending,
    Passing,
    Failing,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrInfo {
    pub number: u64,
    pub title: String,
    pub url: String,
    /// OPEN | MERGED | CLOSED
    pub state: String,
    pub is_draft: bool,
    /// APPROVED | CHANGES_REQUESTED | REVIEW_REQUIRED | ""
    #[serde(default)]
    pub review_decision: String,
    #[serde(default)]
    pub has_feedback: bool,
    pub checks_status: ChecksStatus,
    #[serde(default)]
    pub active_check_url: Option<String>,
    #[serde(default)]
    pub active_check_name: Option<String>,
    #[serde(default)]
    pub checks_waiting: bool,
    /// Branch the PR merges into; another worktree's branch means a stack.
    /// (Rust-only; absent in payloads written by the TS app.)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_ref_name: Option<String>,
    /// MERGEABLE | CONFLICTING | UNKNOWN (GitHub computes it lazily).
    /// (Rust-only; absent in payloads written by the TS app.)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mergeable: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinearState {
    pub name: String,
    #[serde(default)]
    pub color: String,
    #[serde(default, rename = "type")]
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinearProject {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinearPrAttachment {
    pub url: String,
    pub title: String,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinearInfo {
    pub identifier: String,
    pub title: String,
    pub url: String,
    pub state: LinearState,
    #[serde(default)]
    pub priority_label: String,
    #[serde(default)]
    pub assignee: Option<String>,
    #[serde(default)]
    pub project: Option<LinearProject>,
    #[serde(default)]
    pub pr_attachment: Option<LinearPrAttachment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningProcess {
    pub pid: i32,
    /// Friendly label, e.g. `pnpm dev`.
    pub command: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ide {
    Cursor,
    Vscode,
}

/// A subagent row from the `subagents` table, joined with live transcript data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subagent {
    pub session_id: String,
    pub agent_id: String,
    pub agent_type: Option<String>,
    pub description: Option<String>,
    pub running: bool,
    pub tool_count: i64,
    pub last_tool: Option<String>,
    pub last_tool_input: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub updated_at: String,
    pub transcript_path: Option<String>,
}

/// Everything the dashboard shows for one worktree.
#[derive(Debug, Clone, PartialEq)]
pub struct WorktreeView {
    pub worktree: Worktree,
    pub agent: Option<AgentState>,
    pub git: Option<GitStatus>,
    pub last_commit: Option<CommitInfo>,
    pub pr: Option<PrInfo>,
    pub linear: Option<LinearInfo>,
    pub has_terminal: bool,
    pub open_ide: Option<Ide>,
    pub running_processes: Vec<RunningProcess>,
    /// Context window used by the agent's session (from its transcript).
    pub context: Option<crate::context::ContextUsage>,
    /// Live agent sessions here, oldest first. The dashboard lists them under
    /// the row when there are two or more; `agent` is their headline.
    pub sessions: Vec<SessionView>,
}

/// One live agent session in a worktree, with its own status.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionView {
    pub agent: HarnessKind,
    pub session_id: String,
    pub status: Status,
    pub last_response: Option<String>,
    pub started_at: String,
    pub updated_at: String,
    pub context: Option<crate::context::ContextUsage>,
}

impl WorktreeView {
    pub fn status(&self) -> Option<Status> {
        self.agent.as_ref().map(|a| a.status)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct WorktreeGroup {
    pub repo: Repository,
    pub worktrees: Vec<WorktreeView>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_payloads_written_by_the_ts_app_deserialize() {
        let pr = r#"{"number":127,"title":"t","url":"u","state":"OPEN","isDraft":false,"reviewDecision":"","hasFeedback":false,"checksStatus":"passing","activeCheckUrl":null,"activeCheckName":null,"checksWaiting":false}"#;
        let pr: PrInfo = serde_json::from_str(pr).unwrap();
        assert_eq!(pr.checks_status, ChecksStatus::Passing);

        let li = r##"{"identifier":"ENG-1","title":"t","url":"u","state":{"name":"In Progress","color":"#f00","type":"started"},"priorityLabel":"High","assignee":null,"project":{"id":"p","name":"Q4"},"prAttachment":null}"##;
        let li: LinearInfo = serde_json::from_str(li).unwrap();
        assert_eq!(li.project.unwrap().name, "Q4");
        assert_eq!(li.state.kind, "started");
    }
}
