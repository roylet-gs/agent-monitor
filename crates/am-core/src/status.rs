//! Agent-status rules: map a canonical hook event onto a worktree status.
//!
//! The canonical event vocabulary is Claude Code's; other agents' hooks are
//! translated into it by their provider (`crate::harness`) before they get here.
//!
//! Ported from `src/commands/hook-event.ts` (`mapEventToStatus`) with one
//! structural change: events are classified by *origin*. Claude Code tags
//! tool events fired inside a subagent with `agent_id`/`agent_type`, and those
//! events no longer touch the parent worktree's status — they only update the
//! subagent's own row. The old counter-based design let a subagent's
//! PreToolUse overwrite `delegating` with `executing`, and let stray
//! SubagentStops (internal agents report an empty `agent_type`) drive the
//! counter to 0 while real subagents were still running.

use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Idle,
    Executing,
    Planning,
    Waiting,
    Delegating,
    Done,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Idle => "idle",
            Status::Executing => "executing",
            Status::Planning => "planning",
            Status::Waiting => "waiting",
            Status::Delegating => "delegating",
            Status::Done => "done",
        }
    }

    pub fn parse(s: &str) -> Option<Status> {
        Some(match s {
            "idle" => Status::Idle,
            "executing" => Status::Executing,
            "planning" => Status::Planning,
            "waiting" => Status::Waiting,
            "delegating" => Status::Delegating,
            "done" => Status::Done,
            _ => return None,
        })
    }
}

/// The subset of the hook payload the rules look at.
#[derive(Debug, Clone, Default)]
pub struct HookEvent {
    pub event: String,
    pub session_id: Option<String>,
    pub transcript_path: Option<String>,
    pub cwd: Option<String>,
    pub permission_mode: Option<String>,
    pub tool_name: Option<String>,
    pub tool_input: Option<Value>,
    pub stop_hook_active: bool,
    pub last_assistant_message: Option<String>,
    pub transcript_summary: Option<String>,
    pub message: Option<String>,
    pub notification_type: Option<String>,
    pub permission_prompt: bool,
    pub agent_id: Option<String>,
    pub agent_type: Option<String>,
    pub agent_transcript_path: Option<String>,
}

impl HookEvent {
    /// Build from the raw stdin JSON. `event_override` (the `--event` flag) wins
    /// over `hook_event_name`, matching the TS behaviour.
    pub fn from_json(v: &Value, event_override: Option<&str>) -> HookEvent {
        let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_owned);
        let b = |k: &str| v.get(k).and_then(Value::as_bool).unwrap_or(false);
        HookEvent {
            event: event_override
                .map(str::to_owned)
                .or_else(|| s("event"))
                .or_else(|| s("hook_event_name"))
                .unwrap_or_else(|| "unknown".into()),
            session_id: s("session_id"),
            transcript_path: s("transcript_path"),
            cwd: s("cwd"),
            permission_mode: s("permission_mode"),
            tool_name: s("tool_name"),
            tool_input: v.get("tool_input").cloned(),
            stop_hook_active: b("stop_hook_active"),
            last_assistant_message: s("last_assistant_message"),
            transcript_summary: s("transcript_summary"),
            message: s("message"),
            notification_type: s("notification_type"),
            permission_prompt: b("permission_prompt"),
            agent_id: s("agent_id").filter(|a| !a.is_empty()),
            agent_type: s("agent_type"),
            agent_transcript_path: s("agent_transcript_path"),
        }
    }

    pub fn is(&self, name: &str) -> bool {
        self.event == name
    }

    fn tool(&self, name: &str) -> bool {
        self.tool_name.as_deref() == Some(name)
    }

    fn plan_mode(&self) -> bool {
        self.permission_mode.as_deref() == Some("plan")
    }

    pub fn is_subagent_lifecycle(&self) -> bool {
        self.is("SubagentStart") || self.is("SubagentStop")
    }

    /// True when this event was fired from *inside* a subagent (a tool call the
    /// subagent made), as opposed to the main conversation. Lifecycle events
    /// carry `agent_id` too but describe the subagent from the parent's side.
    pub fn from_subagent(&self) -> bool {
        self.agent_id.is_some() && !self.is_subagent_lifecycle()
    }

    /// Internal agents (prompt suggestions, /btw…) report an empty agent_type.
    /// They aren't user-visible work and must not count as running subagents.
    pub fn is_internal_agent(&self) -> bool {
        self.agent_type.as_deref().is_some_and(str::is_empty)
    }

    pub fn needs_user(&self) -> bool {
        self.is("PermissionRequest")
            || (self.is("Notification")
                && matches!(
                    self.notification_type.as_deref(),
                    Some("permission_prompt") | Some("elicitation_dialog")
                ))
            || (self.is("PreToolUse") && self.permission_prompt)
    }
}

/// Map an event to the worktree's new status. `None` means "leave it alone".
/// `running_subagents` is the number of live (non-internal) subagents for the
/// worktree *after* this event's bookkeeping has been applied.
/// Tool names the status rules and summaries know (Claude Code's). A
/// harness plugin maps its own tool names onto these in `tools`.
pub const CANONICAL_TOOLS: [&str; 18] = [
    "Bash", "Read", "Write", "Edit", "MultiEdit", "NotebookEdit", "Glob", "Grep", "LS", "WebFetch", "WebSearch",
    "AskUserQuestion", "EnterPlanMode", "ExitPlanMode", "Agent", "Task", "TodoWrite", "Skill",
];

pub fn map_event_to_status(
    event: &HookEvent,
    current: Option<Status>,
    running_subagents: u32,
) -> Option<Status> {
    use Status::*;

    // Events from inside a subagent never move the parent, except when the
    // subagent is blocked on the user (permission prompt) — that needs eyes.
    if event.from_subagent() {
        return if event.needs_user() { Some(Waiting) } else { None };
    }

    // Protect "done" from late subagent lifecycle and informational events.
    // Main-agent tool events are allowed through: Claude Code starts a new main
    // turn without UserPromptSubmit when a background task notifies it.
    if current == Some(Done) {
        let allowed = matches!(
            event.event.as_str(),
            "UserPromptSubmit" | "SessionStart" | "SessionEnd" | "Stop" | "StopFailure" | "PreToolUse" | "PostToolUse"
        ) || (event.is("SubagentStart") && running_subagents > 0);
        if !allowed {
            return None;
        }
        if event.is("SubagentStart") {
            // A finished session whose subagent was resumed (SendMessage).
            return Some(Delegating);
        }
    }

    // Main turn has ended but subagents are still running.
    if current == Some(Delegating) {
        if event.is_subagent_lifecycle() {
            return Some(if running_subagents > 0 { Delegating } else { Done });
        }
        if event.is("Stop") {
            if event.stop_hook_active {
                return Some(Waiting);
            }
            return Some(if running_subagents > 0 { Delegating } else { Done });
        }
    }

    match event.event.as_str() {
        "Stop" => {
            if event.stop_hook_active {
                return Some(Waiting);
            }
            if running_subagents > 0 {
                return Some(Delegating);
            }
            if current == Some(Waiting) {
                return Some(Waiting);
            }
            if event.plan_mode() || current == Some(Planning) {
                return Some(Waiting);
            }
            if current == Some(Executing) {
                return Some(Done);
            }
            return Some(Idle);
        }
        "StopFailure" => return Some(Idle),
        "Notification" => {
            return match event.notification_type.as_deref() {
                Some("permission_prompt") | Some("elicitation_dialog") => Some(Waiting),
                Some("idle_prompt") => {
                    if current == Some(Done) {
                        None
                    } else if running_subagents > 0 {
                        Some(Delegating)
                    } else {
                        Some(Waiting)
                    }
                }
                _ => None,
            };
        }
        "PermissionRequest" => return Some(Waiting),
        "SessionStart" | "SessionEnd" => return Some(Idle),
        "UserPromptSubmit" => return Some(if event.plan_mode() { Planning } else { Executing }),
        _ => {}
    }

    if event.tool("AskUserQuestion") || event.tool("ExitPlanMode") {
        return Some(Waiting);
    }
    if event.tool("EnterPlanMode") {
        return Some(Planning);
    }
    if event.is("PreToolUse") && event.permission_prompt {
        return Some(Waiting);
    }
    if event.plan_mode() {
        return Some(Planning);
    }

    let working = matches!(
        event.event.as_str(),
        "PreToolUse" | "PostToolUse" | "SubagentStart" | "SubagentStop"
    );
    if working {
        return Some(if current == Some(Planning) { Planning } else { Executing });
    }
    Some(Idle)
}

/// `git push` / `gh pr create` in a Bash command → targeted PR refresh hint.
pub fn detect_git_activity(command: &str) -> Option<&'static str> {
    let words: Vec<&str> = command.split(|c: char| c.is_whitespace() || c == ';' || c == '&' || c == '|').filter(|w| !w.is_empty()).collect();
    for w in words.windows(3) {
        if w[0] == "gh" && w[1] == "pr" && w[2] == "create" {
            return Some("pr-create");
        }
    }
    for w in words.windows(2) {
        if w[0] == "git" && w[1] == "push" {
            return Some("push");
        }
    }
    None
}

/// A short, single-line summary of a tool's input for display
/// (`Bash: pnpm test`, `Read: src/lib/db.ts`). Never the full payload.
pub fn summarize_tool_input(tool: &str, input: Option<&Value>, cwd: Option<&str>) -> Option<String> {
    let input = input?;
    let pick = |k: &str| input.get(k).and_then(Value::as_str);
    let raw = match tool {
        "Bash" => pick("description").or(pick("command")),
        "Read" | "Edit" | "Write" | "NotebookEdit" => pick("file_path").or(pick("notebook_path")),
        "Grep" | "Glob" => pick("pattern"),
        "WebFetch" => pick("url").or(pick("prompt")),
        "WebSearch" => pick("query"),
        "Agent" | "Task" => pick("description").or(pick("agent_name")),
        _ => None,
    }?;
    let line = raw.lines().next().unwrap_or("").trim();
    // Paths inside the session's directory read better relative to it.
    let line = cwd
        .filter(|c| !c.is_empty())
        .and_then(|c| line.strip_prefix(c))
        .map(|rest| rest.trim_start_matches('/'))
        .filter(|rest| !rest.is_empty())
        .unwrap_or(line);
    Some(truncate(line, 120))
}

pub fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use Status::*;

    fn ev(name: &str) -> HookEvent {
        HookEvent { event: name.into(), ..Default::default() }
    }
    fn with(mut e: HookEvent, f: impl FnOnce(&mut HookEvent)) -> HookEvent {
        f(&mut e);
        e
    }
    fn map(e: &HookEvent, cur: Option<Status>) -> Option<Status> {
        map_event_to_status(e, cur, 0)
    }
    fn tool(name: &str, t: &str) -> HookEvent {
        with(ev(name), |e| e.tool_name = Some(t.into()))
    }
    fn sub(e: HookEvent) -> HookEvent {
        with(e, |e| {
            e.agent_id = Some("a1".into());
            e.agent_type = Some("Explore".into());
        })
    }

    // --- Ported from tests/commands/hook-event.test.ts ---

    #[test]
    fn stop_rules() {
        assert_eq!(map(&ev("Stop"), None), Some(Idle));
        assert_eq!(map(&with(ev("Stop"), |e| e.stop_hook_active = true), None), Some(Waiting));
        assert_eq!(map(&ev("Stop"), Some(Executing)), Some(Done));
        assert_eq!(map(&ev("Stop"), Some(Planning)), Some(Waiting));
        assert_eq!(map(&ev("Stop"), Some(Waiting)), Some(Waiting));
        let plan = with(ev("Stop"), |e| e.permission_mode = Some("plan".into()));
        assert_eq!(map(&plan, Some(Executing)), Some(Waiting));
        assert_eq!(map(&plan, None), Some(Waiting));
        assert_eq!(map(&ev("Stop"), Some(Idle)), Some(Idle));
        assert_eq!(map(&ev("Stop"), Some(Done)), Some(Idle));
        assert_eq!(map(&with(ev("Stop"), |e| e.stop_hook_active = true), Some(Done)), Some(Waiting));
        let accept = with(ev("Stop"), |e| e.permission_mode = Some("acceptEdits".into()));
        assert_eq!(map(&accept, None), Some(Idle));
        assert_eq!(map(&accept, Some(Executing)), Some(Done));
    }

    #[test]
    fn notification_rules() {
        let n = |t: &str| with(ev("Notification"), |e| e.notification_type = Some(t.into()));
        assert_eq!(map(&n("permission_prompt"), None), Some(Waiting));
        assert_eq!(map(&n("elicitation_dialog"), None), Some(Waiting));
        assert_eq!(map(&ev("Notification"), None), None);
        assert_eq!(map(&n("idle_prompt"), None), Some(Waiting));
        assert_eq!(map(&n("idle_prompt"), Some(Done)), None);
        assert_eq!(map(&n("auth_success"), None), None);
        assert_eq!(map(&ev("Notification"), Some(Done)), None);
    }

    #[test]
    fn lifecycle_and_prompt_rules() {
        assert_eq!(map(&ev("PermissionRequest"), None), Some(Waiting));
        assert_eq!(map(&ev("PermissionRequest"), Some(Executing)), Some(Waiting));
        assert_eq!(map(&ev("PermissionRequest"), Some(Done)), None);
        assert_eq!(map(&ev("SessionStart"), None), Some(Idle));
        assert_eq!(map(&ev("SessionEnd"), Some(Done)), Some(Idle));
        assert_eq!(map(&ev("UserPromptSubmit"), None), Some(Executing));
        assert_eq!(map(&ev("UserPromptSubmit"), Some(Done)), Some(Executing));
        let plan = with(ev("UserPromptSubmit"), |e| e.permission_mode = Some("plan".into()));
        assert_eq!(map(&plan, None), Some(Planning));
        assert_eq!(map(&ev("StopFailure"), Some(Executing)), Some(Idle));
        assert_eq!(map(&ev("StopFailure"), Some(Done)), Some(Idle));
        assert_eq!(map(&ev("SomethingNew"), None), Some(Idle));
    }

    #[test]
    fn tool_rules() {
        assert_eq!(map(&tool("PreToolUse", "AskUserQuestion"), None), Some(Waiting));
        assert_eq!(map(&tool("PreToolUse", "ExitPlanMode"), None), Some(Waiting));
        assert_eq!(map(&tool("PreToolUse", "EnterPlanMode"), None), Some(Planning));
        assert_eq!(map(&tool("PostToolUse", "EnterPlanMode"), None), Some(Planning));
        assert_eq!(map(&ev("PreToolUse"), None), Some(Executing));
        assert_eq!(map(&ev("PostToolUse"), Some(Executing)), Some(Executing));
        assert_eq!(map(&ev("PreToolUse"), Some(Planning)), Some(Planning));
        assert_eq!(map(&ev("SubagentStart"), Some(Planning)), Some(Planning));
        assert_eq!(map(&ev("SubagentStart"), None), Some(Executing));
        let plan = with(ev("PreToolUse"), |e| e.permission_mode = Some("plan".into()));
        assert_eq!(map(&plan, None), Some(Planning));
        let prompt = with(ev("PreToolUse"), |e| e.permission_prompt = true);
        assert_eq!(map(&prompt, None), Some(Waiting));
        let prompt_plan = with(prompt.clone(), |e| e.permission_mode = Some("plan".into()));
        assert_eq!(map(&prompt_plan, None), Some(Waiting));
    }

    #[test]
    fn plan_mode_flow() {
        let s = map(&tool("PreToolUse", "EnterPlanMode"), Some(Executing));
        assert_eq!(s, Some(Planning));
        let s = map(&ev("PreToolUse"), s);
        let s = map(&ev("PostToolUse"), s);
        assert_eq!(s, Some(Planning));
        assert_eq!(map(&ev("Stop"), s), Some(Waiting));
    }

    #[test]
    fn done_is_protected_from_late_subagent_events() {
        assert_eq!(map(&ev("SubagentStop"), Some(Done)), None);
        assert_eq!(map(&ev("SubagentStart"), Some(Done)), None);
        assert_eq!(map(&sub(ev("PreToolUse")), Some(Done)), None);
        assert_eq!(map(&sub(ev("PostToolUse")), Some(Done)), None);
    }

    #[test]
    fn delegating_rules() {
        let m = |e: &HookEvent, cur, n| map_event_to_status(e, cur, n);
        assert_eq!(m(&ev("Stop"), Some(Executing), 1), Some(Delegating));
        let plan = with(ev("Stop"), |e| e.permission_mode = Some("plan".into()));
        assert_eq!(m(&plan, Some(Planning), 2), Some(Delegating));
        assert_eq!(m(&with(ev("Stop"), |e| e.stop_hook_active = true), Some(Executing), 1), Some(Waiting));
        let idle = with(ev("Notification"), |e| e.notification_type = Some("idle_prompt".into()));
        assert_eq!(m(&idle, Some(Executing), 1), Some(Delegating));
        assert_eq!(m(&ev("PermissionRequest"), Some(Delegating), 1), Some(Waiting));
        assert_eq!(m(&ev("SubagentStop"), Some(Delegating), 1), Some(Delegating));
        assert_eq!(m(&ev("SubagentStop"), Some(Delegating), 0), Some(Done));
        assert_eq!(m(&ev("SubagentStart"), Some(Delegating), 2), Some(Delegating));
        assert_eq!(m(&ev("Stop"), Some(Delegating), 1), Some(Delegating));
        assert_eq!(m(&ev("Stop"), Some(Delegating), 0), Some(Done));
        assert_eq!(m(&ev("UserPromptSubmit"), Some(Delegating), 1), Some(Executing));
        assert_eq!(m(&ev("PreToolUse"), Some(Delegating), 1), Some(Executing));
        assert_eq!(m(&ev("Stop"), Some(Executing), 0), Some(Done));
    }

    // --- New origin-aware behaviour ---

    #[test]
    fn subagent_tool_events_do_not_move_the_parent() {
        // The bug from the logs: a subagent's PreToolUse overwrote delegating.
        assert_eq!(map_event_to_status(&sub(ev("PreToolUse")), Some(Delegating), 3), None);
        assert_eq!(map_event_to_status(&sub(ev("PostToolUse")), Some(Executing), 1), None);
        assert_eq!(map_event_to_status(&sub(tool("PreToolUse", "AskUserQuestion")), Some(Delegating), 1), None);
    }

    #[test]
    fn subagent_blocked_on_user_surfaces_as_waiting() {
        assert_eq!(map_event_to_status(&sub(ev("PermissionRequest")), Some(Delegating), 1), Some(Waiting));
    }

    #[test]
    fn main_agent_tool_event_after_done_resumes() {
        // Background-task notification wakes the main agent without a prompt.
        assert_eq!(map(&ev("PreToolUse"), Some(Done)), Some(Executing));
    }

    #[test]
    fn resumed_subagent_after_done_is_delegating() {
        assert_eq!(map_event_to_status(&ev("SubagentStart"), Some(Done), 1), Some(Delegating));
    }

    #[test]
    fn origin_classification() {
        assert!(sub(ev("PreToolUse")).from_subagent());
        assert!(!sub(ev("SubagentStop")).from_subagent());
        assert!(!ev("PreToolUse").from_subagent());
        let internal = with(ev("SubagentStop"), |e| {
            e.agent_id = Some("x".into());
            e.agent_type = Some(String::new());
        });
        assert!(internal.is_internal_agent());
    }

    #[test]
    fn git_activity() {
        assert_eq!(detect_git_activity("git push"), Some("push"));
        assert_eq!(detect_git_activity("git push -u origin feat"), Some("push"));
        assert_eq!(detect_git_activity("pnpm test && gh pr create --fill"), Some("pr-create"));
        assert_eq!(detect_git_activity("git pull"), None);
        assert_eq!(detect_git_activity(""), None);
        assert_eq!(detect_git_activity("echo"), None);
    }

    #[test]
    fn tool_input_summary_is_one_short_line() {
        let v = serde_json::json!({"command": "pnpm test\n--watch"});
        assert_eq!(summarize_tool_input("Bash", Some(&v), None).as_deref(), Some("pnpm test"));
        let v = serde_json::json!({"file_path": "/a/b.rs"});
        assert_eq!(summarize_tool_input("Read", Some(&v), None).as_deref(), Some("/a/b.rs"));
        assert_eq!(summarize_tool_input("Read", Some(&v), Some("/a")).as_deref(), Some("b.rs"));
        assert_eq!(summarize_tool_input("Read", Some(&v), Some("/x")).as_deref(), Some("/a/b.rs"));
        assert_eq!(summarize_tool_input("Mystery", Some(&v), None), None);
        assert_eq!(truncate("abcdef", 4), "abc…");
    }
}
