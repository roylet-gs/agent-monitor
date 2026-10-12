//! Claude Code's own session list, `claude agents --json`: every active
//! session, interactive and background (`claude --bg`, run by Claude's
//! supervisor). Hooks already cover interactive sessions; this adds the
//! background ones: a `bg` badge, `claude attach <id>` as the open action,
//! and their state when hooks have nothing (or nothing open) for that
//! worktree. Background sessions outside any tracked worktree become
//! "Other sessions" rows.
//!
//! Status rules (see STATUS_MAPPING.md): `status` busy → executing,
//! waiting → waiting, idle → idle; else `state` working → executing,
//! blocked → waiting, done → done, failed → done (flagged); stopped and
//! anything unknown are ignored.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::model::{AgentState, BackgroundAgent, WorktreeView};
use crate::status::Status;

const TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Agent {
    pub id: String,
    #[serde(default)]
    pub session_id: Option<String>,
    pub cwd: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub waiting_for: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    /// Unix milliseconds.
    #[serde(default)]
    pub started_at: i64,
}

impl Agent {
    pub fn is_background(&self) -> bool {
        self.kind == "background"
    }

    pub fn am_status(&self) -> Option<Status> {
        if let Some(s) = self.status.as_deref() {
            match s {
                "busy" => return Some(Status::Executing),
                "waiting" => return Some(Status::Waiting),
                "idle" => return Some(Status::Idle),
                _ => {}
            }
        }
        match self.state.as_deref()? {
            "working" | "running" => Some(Status::Executing),
            "blocked" | "waiting" => Some(Status::Waiting),
            "done" | "failed" => Some(Status::Done),
            _ => None,
        }
    }

    pub fn info(&self) -> BackgroundAgent {
        BackgroundAgent {
            id: self.id.clone(),
            name: self.name.clone(),
            failed: self.state.as_deref() == Some("failed"),
            waiting_for: self.waiting_for.clone(),
        }
    }
}

pub fn parse(json: &str) -> Result<Vec<Agent>, String> {
    serde_json::from_str(json).map_err(|e| format!("unexpected `claude agents --json` output: {e}"))
}

fn claude_bin() -> PathBuf {
    let on_path = std::env::var_os("PATH")
        .and_then(|p| std::env::split_paths(&p).map(|d| d.join("claude")).find(|p| p.is_file()));
    // GUI-launched terminals and tmux servers may start with a short PATH.
    let home = crate::paths::home_dir();
    on_path
        .or_else(|| [home.join(".local/bin/claude"), home.join(".claude/local/claude")].into_iter().find(|p| p.is_file()))
        .unwrap_or_else(|| PathBuf::from("claude"))
}

/// Run `claude agents --json` (active sessions only), killed after 3 s.
pub fn fetch() -> Result<Vec<Agent>, String> {
    let mut child = Command::new(claude_bin())
        .args(["agents", "--json"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("can't run claude: {e}"))?;
    let mut stdout = child.stdout.take().ok_or("no stdout")?;
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stdout.read_to_string(&mut s);
        s
    });
    let started = Instant::now();
    let status = loop {
        if let Some(st) = child.try_wait().map_err(|e| e.to_string())? {
            break st;
        }
        if started.elapsed() > TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return Err("claude agents timed out".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let out = reader.join().unwrap_or_default();
    if !status.success() {
        return Err(format!("claude agents exited with {status}"));
    }
    parse(&out)
}

fn within(cwd: &str, root: &str) -> bool {
    let root = root.trim_end_matches('/');
    cwd.strip_prefix(root).is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// Index of the most specific worktree root containing `cwd` (worktrees
/// nested in the main checkout win over it).
pub fn attribute(cwd: &str, views: &[WorktreeView]) -> Option<usize> {
    views.iter().enumerate().filter(|(_, v)| within(cwd, &v.worktree.path)).max_by_key(|(_, v)| v.worktree.path.len()).map(|(i, _)| i)
}

fn priority(s: Option<Status>) -> u8 {
    match s {
        Some(Status::Waiting) => 3,
        Some(Status::Executing | Status::Planning | Status::Delegating) => 2,
        Some(Status::Done) => 1,
        _ => 0,
    }
}

/// A state for a session hooks didn't report (or reported as closed),
/// stamped with its start time so it stays stable across polls.
pub fn agent_state(a: &Agent) -> Option<AgentState> {
    Some(AgentState {
        status: a.am_status()?,
        session_id: a.session_id.clone(),
        last_response: None,
        is_open: true,
        active_subagents: 0,
        updated_at: crate::timefmt::sqlite_from_unix(a.started_at / 1000),
    })
}

/// Attach background agents to their worktree views; returns the ones that
/// match no tracked worktree. Hook state stays primary while that session
/// is open (it knows planning / delegating / subagents); otherwise the
/// background agent's state shows. With several in one worktree, the one
/// needing attention wins.
pub fn merge(views: &mut [WorktreeView], agents: &[Agent]) -> Vec<Agent> {
    let mut unmatched = Vec::new();
    let mut best: std::collections::HashMap<usize, &Agent> = std::collections::HashMap::new();
    for a in agents.iter().filter(|a| a.is_background() && a.am_status().is_some()) {
        match attribute(&a.cwd, views) {
            Some(i) => {
                if best.get(&i).is_none_or(|b| priority(a.am_status()) > priority(b.am_status())) {
                    best.insert(i, a);
                }
            }
            None => unmatched.push(a.clone()),
        }
    }
    for (i, a) in best {
        let v = &mut views[i];
        v.background = Some(a.info());
        if !crate::grouping::is_open_now(v.agent.as_ref()) {
            v.agent = agent_state(a);
        }
    }
    unmatched
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Worktree;

    const FIXTURE: &str = include_str!("fixtures/claude_agents.json");

    fn view(path: &str) -> WorktreeView {
        WorktreeView {
            worktree: Worktree {
                id: path.into(),
                repo_id: "r".into(),
                path: path.into(),
                branch: path.into(),
                name: path.into(),
                custom_name: None,
                is_main: false,
                created_at: String::new(),
            },
            agent: None,
            git: None,
            last_commit: None,
            pr: None,
            linear: None,
            has_terminal: false,
            open_ide: None,
            running_processes: Vec::new(),
            background: None,
        }
    }

    #[test]
    fn parses_real_output_and_maps_status() {
        let agents = parse(FIXTURE).unwrap();
        assert_eq!(agents.len(), 6);
        let by_id = |id: &str| agents.iter().find(|a| a.id == id).unwrap();
        assert_eq!(by_id("bg-work").am_status(), Some(Status::Executing));
        assert_eq!(by_id("bg-blocked").am_status(), Some(Status::Waiting));
        assert_eq!(by_id("bg-failed").am_status(), Some(Status::Done));
        assert!(by_id("bg-failed").info().failed);
        assert_eq!(by_id("bg-stopped").am_status(), None);
        assert_eq!(by_id("f79c7d58").am_status(), Some(Status::Waiting));
        assert_eq!(by_id("f79c7d58").waiting_for.as_deref(), Some("permission prompt"));
        assert!(!by_id("f79c7d58").is_background());
        assert!(parse("not json").is_err());
    }

    #[test]
    fn attributes_to_the_most_specific_worktree() {
        let views = vec![view("/repo"), view("/repo/.claude/worktrees/feat"), view("/repo-other")];
        assert_eq!(attribute("/repo/.claude/worktrees/feat/src", &views), Some(1));
        assert_eq!(attribute("/repo/src", &views), Some(0));
        assert_eq!(attribute("/repo-other", &views), Some(2));
        assert_eq!(attribute("/elsewhere", &views), None);
    }

    #[test]
    fn merge_badges_fills_closed_state_and_returns_unmatched() {
        let agents = parse(FIXTURE).unwrap();
        let mut views = vec![view("/repo"), view("/repo/.claude/worktrees/feat")];
        // /repo has an open hook session: its status stays.
        views[0].agent = Some(AgentState {
            status: Status::Planning,
            session_id: Some("s".into()),
            last_response: None,
            is_open: true,
            active_subagents: 0,
            updated_at: "9999-01-01 00:00:00".into(),
        });
        let unmatched = merge(&mut views, &agents);
        assert_eq!(views[0].status(), Some(Status::Planning));
        assert_eq!(views[0].background.as_ref().map(|b| b.id.as_str()), Some("bg-work"));
        // feat has a working and a blocked agent: the blocked one needs you.
        assert_eq!(views[1].background.as_ref().map(|b| b.id.as_str()), Some("bg-blocked"));
        assert_eq!(views[1].status(), Some(Status::Waiting));
        assert_eq!(views[1].agent.as_ref().unwrap().session_id.as_deref(), Some("sess-blocked"));
        assert_eq!(views[1].agent.as_ref().unwrap().updated_at, "2026-09-17 00:42:02", "startedAt, not now");
        // Interactive and stopped agents are never merged; failed elsewhere is unmatched.
        let ids: Vec<&str> = unmatched.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["bg-failed"]);
    }
}
