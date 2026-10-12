//! `am summary`: agent counts for status bars (tmux `status-right`, shell
//! prompts, WezTerm/zjstatus segments). Runs every few seconds, so it only
//! reads SQLite: no git, no network, no `claude agents`.

use am_core::grouping;
use am_core::model::AgentState;
use am_core::status::Status;

use crate::store;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub waiting: usize,
    pub executing: usize,
    pub planning: usize,
    pub delegating: usize,
    pub done: usize,
    pub idle: usize,
    pub total: usize,
    pub needs_attention: Vec<Attention>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Attention {
    pub name: String,
    pub path: String,
}

/// `(display name, path, state)` of every tracked session; only sessions
/// that are open right now count.
pub fn summarize(sessions: &[(String, String, AgentState)]) -> Summary {
    let mut s = Summary::default();
    for (name, path, a) in sessions.iter().filter(|(_, _, a)| grouping::is_open_now(Some(a))) {
        s.total += 1;
        match a.status {
            Status::Waiting => {
                s.waiting += 1;
                s.needs_attention.push(Attention { name: name.clone(), path: path.clone() });
            }
            Status::Executing => s.executing += 1,
            Status::Planning => s.planning += 1,
            Status::Delegating => s.delegating += 1,
            Status::Done => s.done += 1,
            Status::Idle => s.idle += 1,
        }
    }
    s
}

impl Summary {
    pub fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "waiting": self.waiting,
            "executing": self.executing,
            "planning": self.planning,
            "delegating": self.delegating,
            "done": self.done,
            "idle": self.idle,
            "total": self.total,
            "needsAttention": self.needs_attention.iter().map(|a| serde_json::json!({ "name": a.name, "path": a.path })).collect::<Vec<_>>(),
        })
    }
}

fn segments(s: &Summary) -> Vec<(&'static str, String)> {
    [
        ("yellow", "◐", s.waiting),
        ("green", "●", s.executing + s.planning),
        ("green", "◑", s.delegating),
        ("blue", "✓", s.done),
    ]
    .into_iter()
    .filter(|(_, _, n)| *n > 0)
    .map(|(color, glyph, n)| (color, format!("{glyph}{n}")))
    .collect()
}

/// tmux status format: coloured, empty when nothing is going on.
pub fn tmux(s: &Summary) -> String {
    let segs = segments(s);
    if segs.is_empty() {
        return String::new();
    }
    let body: Vec<String> = segs.into_iter().map(|(c, t)| format!("#[fg={c}]{t}")).collect();
    format!("{}#[default]", body.join(" "))
}

pub fn plain(s: &Summary) -> String {
    segments(s).into_iter().map(|(_, t)| t).collect::<Vec<_>>().join(" ")
}

fn sessions(conn: &rusqlite::Connection) -> rusqlite::Result<Vec<(String, String, AgentState)>> {
    let mut agents = am_core::db::agent_states(conn)?;
    let mut out: Vec<(String, String, AgentState)> = am_core::db::worktrees(conn)?
        .into_iter()
        .filter_map(|w| {
            let a = agents.remove(&w.id)?;
            Some((w.display_name().to_string(), w.path, a))
        })
        .collect();
    let home = am_core::paths::home_dir().to_string_lossy().into_owned();
    for (_, path, a) in am_core::db::standalone_sessions(conn)? {
        let name = path.strip_prefix(&home).map(|p| format!("~{p}")).unwrap_or_else(|| path.clone());
        out.push((name, path, a));
    }
    Ok(out)
}

pub fn run(json: bool, format: &str) {
    let conn = store::open();
    let s = summarize(&sessions(&conn).unwrap_or_else(|e| crate::fail(&format!("Failed to read agent status: {e}"))));
    if json {
        crate::output::json(&s.json());
        return;
    }
    match format {
        "tmux" => println!("{}", tmux(&s)),
        "plain" => println!("{}", plain(&s)),
        other => crate::fail(&format!("Unknown format '{other}' (expected plain or tmux)")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(status: Status, open: bool) -> AgentState {
        AgentState {
            status,
            session_id: None,
            last_response: None,
            is_open: open,
            active_subagents: 0,
            updated_at: "9999-12-31 23:59:59".into(),
        }
    }

    #[test]
    fn counts_open_sessions_only() {
        let rows = vec![
            ("a".into(), "/a".into(), st(Status::Waiting, true)),
            ("b".into(), "/b".into(), st(Status::Executing, true)),
            ("c".into(), "/c".into(), st(Status::Planning, true)),
            ("d".into(), "/d".into(), st(Status::Done, true)),
            ("e".into(), "/e".into(), st(Status::Waiting, false)),
        ];
        let s = summarize(&rows);
        assert_eq!((s.waiting, s.executing, s.planning, s.done, s.total), (1, 1, 1, 1, 4));
        assert_eq!(s.needs_attention, vec![Attention { name: "a".into(), path: "/a".into() }]);
        assert_eq!(tmux(&s), "#[fg=yellow]◐1 #[fg=green]●2 #[fg=blue]✓1#[default]");
        assert_eq!(plain(&s), "◐1 ●2 ✓1");
    }

    #[test]
    fn empty_when_nothing_runs() {
        let s = summarize(&[("a".into(), "/a".into(), st(Status::Idle, true))]);
        assert_eq!(s.idle, 1);
        assert_eq!(tmux(&s), "");
        assert_eq!(plain(&s), "");
    }
}
