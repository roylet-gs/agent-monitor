//! Dashboard filtering, sorting and per-repo grouping, ported from
//! `src/lib/grouping.ts`. The TUI and any daemon both go through these
//! functions so the visible row order can never diverge between them.
//!
//! Clustering by Linear ticket or project is achieved purely through the sort
//! order (`Settings::worktree_sort`); there is no separate grouping setting.

use std::cmp::Ordering;
use std::collections::HashMap;

use time::OffsetDateTime;
use time::macros::format_description;

use crate::model::{AgentState, ChecksStatus, Repository, WorktreeGroup, WorktreeView};
use crate::settings::{Direction, Settings, SortCriterion, SortKey};
use crate::status::Status;

/// An `is_open` session reporting `idle` counts as closed once it has been
/// quiet this long (mirrors `isEffectivelyOpen` in `agent-utils.ts`).
const IDLE_OPEN_GRACE: time::Duration = time::Duration::minutes(10);

// ---------------------------------------------------------------------------
// Collation
// ---------------------------------------------------------------------------

/// Approximation of JS `String.prototype.localeCompare` (ICU root collation)
/// for the identifiers we sort: case-insensitive at the primary level with
/// punctuation/symbols < digits < letters, then lowercase before uppercase,
/// then a plain code-point compare so the order is total.
fn locale_cmp(a: &str, b: &str) -> Ordering {
    fn class(c: char) -> u8 {
        if c.is_alphabetic() {
            2
        } else if c.is_numeric() {
            1
        } else {
            0
        }
    }
    let primary = |s: &str| {
        s.chars()
            .flat_map(|c| c.to_lowercase().map(move |l| (class(c), l)))
            .collect::<Vec<_>>()
    };
    primary(a)
        .cmp(&primary(b))
        .then_with(|| {
            // Tertiary: lowercase sorts before uppercase at the first case difference.
            let rank = |c: char| if c.is_uppercase() { 1u8 } else { 0 };
            a.chars().map(rank).cmp(b.chars().map(rank))
        })
        .then_with(|| a.cmp(b))
}

/// Present-before-absent string comparator (Linear ticket/project): values
/// that are set sort before empty ones, then by `locale_cmp`.
fn present_first(a: &str, b: &str) -> Ordering {
    match (a.is_empty(), b.is_empty()) {
        _ if a == b => Ordering::Equal,
        (false, true) => Ordering::Less,
        (true, false) => Ordering::Greater,
        _ => locale_cmp(a, b),
    }
}

// ---------------------------------------------------------------------------
// Ranks
// ---------------------------------------------------------------------------

/// Agent status ordering, active-first (lower = higher priority). `delegating`
/// is absent from the TS `AGENT_RANK` table and so falls back to the "none"
/// rank (5); kept identical here so both apps order rows the same way.
fn agent_rank(v: &WorktreeView) -> u8 {
    match v.status() {
        Some(Status::Executing) => 0,
        // Main turn ended but background subagents are still working: active.
        Some(Status::Delegating) => 1,
        Some(Status::Planning) => 2,
        Some(Status::Waiting) => 3,
        Some(Status::Idle) => 4,
        Some(Status::Done) => 5,
        None => 6,
    }
}

/// PR ordering, attention-first: failing checks and requested changes float
/// up, merged/closed sink, worktrees without a PR go last.
fn pr_rank(v: &WorktreeView) -> u8 {
    let Some(pr) = &v.pr else { return 7 };
    if pr.checks_status == ChecksStatus::Failing {
        return 0;
    }
    if pr.has_feedback || pr.review_decision == "CHANGES_REQUESTED" {
        return 1;
    }
    if pr.checks_status == ChecksStatus::Pending || pr.checks_waiting {
        return 2;
    }
    match pr.state.as_str() {
        "OPEN" if pr.is_draft => 4,
        "OPEN" => 3,
        "MERGED" => 5,
        _ => 6, // CLOSED (or any other terminal state)
    }
}

// ---------------------------------------------------------------------------
// Comparator
// ---------------------------------------------------------------------------

/// Ascending comparison for a single key (the `SORT_REGISTRY` entries).
fn compare_key(a: &WorktreeView, b: &WorktreeView, key: SortKey, repo_names: &HashMap<String, String>) -> Ordering {
    fn ticket(v: &WorktreeView) -> &str {
        v.linear.as_ref().map(|l| l.identifier.as_str()).unwrap_or("")
    }
    fn project(v: &WorktreeView) -> &str {
        v.linear
            .as_ref()
            .and_then(|l| l.project.as_ref())
            .map(|p| p.name.as_str())
            .unwrap_or("")
    }
    fn activity(v: &WorktreeView) -> &str {
        v.agent.as_ref().map(|s| s.updated_at.as_str()).unwrap_or("")
    }
    let dirty_rank = |v: &WorktreeView| if v.git.is_some_and(|g| g.dirty > 0) { 0u8 } else { 1 };
    let repo = |v: &WorktreeView| repo_names.get(&v.worktree.repo_id).map(String::as_str).unwrap_or("");

    match key {
        SortKey::IsMain => a.worktree.is_main.cmp(&b.worktree.is_main),
        // Repo is structural: build_groups orders the repo sections. Inside a
        // group every row shares a repo, so this is a no-op there; for a flat
        // cross-repo list it orders by repo name.
        SortKey::Repo => locale_cmp(repo(a), repo(b)),
        SortKey::LinearTicket => present_first(ticket(a), ticket(b)),
        SortKey::LinearProject => present_first(project(a), project(b)),
        SortKey::AgentStatus => agent_rank(a).cmp(&agent_rank(b)),
        SortKey::LastActivity => locale_cmp(activity(a), activity(b)),
        SortKey::CreatedAt => locale_cmp(&a.worktree.created_at, &b.worktree.created_at),
        SortKey::BranchName => locale_cmp(&a.worktree.branch, &b.worktree.branch),
        SortKey::PrStatus => pr_rank(a).cmp(&pr_rank(b)),
        SortKey::GitDirty => dirty_rank(a).cmp(&dirty_rank(b)),
    }
}

/// Compare two worktrees by an ordered list of criteria. Only enabled criteria
/// participate; the first non-equal one decides, reversed for `Desc`.
/// `repo_names` maps repo id → name and is only consulted by `SortKey::Repo`.
pub fn compare(
    a: &WorktreeView,
    b: &WorktreeView,
    criteria: &[SortCriterion],
    repo_names: &HashMap<String, String>,
) -> Ordering {
    for c in criteria.iter().filter(|c| c.enabled) {
        let r = compare_key(a, b, c.key, repo_names);
        if r != Ordering::Equal {
            return match c.direction {
                Direction::Asc => r,
                Direction::Desc => r.reverse(),
            };
        }
    }
    Ordering::Equal
}

/// The first enabled sort criterion's key. The TUI shows Linear-project
/// cluster headers only when this is `LinearProject`.
pub fn top_sort_key(settings: &Settings) -> Option<SortKey> {
    settings.worktree_sort.iter().find(|c| c.enabled).map(|c| c.key)
}

// ---------------------------------------------------------------------------
// Filters
// ---------------------------------------------------------------------------

fn now_sqlite_minus_grace() -> String {
    let fmt = format_description!("[year]-[month]-[day] [hour]:[minute]:[second]");
    (OffsetDateTime::now_utc() - IDLE_OPEN_GRACE).format(&fmt).unwrap_or_default()
}

/// `isEffectivelyOpen`: an open session is open unless it is idle and hasn't
/// been touched within the grace period. `cutoff` is `now - 10min` as SQLite
/// datetime text, so the check is a lexical comparison.
/// Whether an agent session is visibly open right now (used by filters and by
/// the Open action, which skips resuming a session the user can already see).
pub fn is_open_now(agent: Option<&AgentState>) -> bool {
    is_effectively_open(agent, &now_sqlite_minus_grace())
}

fn is_effectively_open(agent: Option<&AgentState>, cutoff: &str) -> bool {
    let Some(a) = agent else { return false };
    if !a.is_open {
        return false;
    }
    if a.status == Status::Idle && a.updated_at.as_str() < cutoff {
        return false;
    }
    true
}

fn apply_filters_with_cutoff(views: Vec<WorktreeView>, settings: &Settings, cutoff: &str) -> Vec<WorktreeView> {
    views
        .into_iter()
        .filter(|v| {
            let open = is_effectively_open(v.agent.as_ref(), cutoff);
            if open {
                // A row with a live session is never hidden.
                return true;
            }
            let wt = &v.worktree;
            if settings.hide_main_branch && wt.is_main && (wt.branch == "main" || wt.branch == "master") {
                return false;
            }
            if settings.hide_merged_closed_prs
                && v.pr.as_ref().is_some_and(|p| p.state == "MERGED" || p.state == "CLOSED")
            {
                return false;
            }
            if settings.hide_idle_done_agents && matches!(v.status(), None | Some(Status::Idle) | Some(Status::Done)) {
                return false;
            }
            if settings.hide_without_linear_ticket && v.linear.as_ref().is_none_or(|l| l.identifier.is_empty()) {
                return false;
            }
            true
        })
        .collect()
}

/// Apply the dashboard's visibility filters (`hideMainBranch`,
/// `hideMergedClosedPrs`, `hideIdleDoneAgents`, `hideWithoutLinearTicket`).
/// A worktree with an effectively-open session is never hidden.
pub fn apply_filters(views: Vec<WorktreeView>, settings: &Settings) -> Vec<WorktreeView> {
    apply_filters_with_cutoff(views, settings, &now_sqlite_minus_grace())
}

// ---------------------------------------------------------------------------
// Groups
// ---------------------------------------------------------------------------

/// Build one group per repo, with worktrees sorted by `settings.worktree_sort`.
///
/// `repos` is in config order. Repo sections keep that order unless a `repo`
/// criterion is enabled, in which case they are ordered by repo name (honoring
/// its direction). Views whose `repo_id` matches no repo are dropped. With a
/// single repo its group is kept even when empty; with several, empty repos
/// are omitted. Filtering is not applied here — call `apply_filters` first.
///
/// Invariant: the concatenation of `groups[].worktrees` is the visual row order.
pub fn build_groups(repos: &[Repository], views: Vec<WorktreeView>, settings: &Settings) -> Vec<WorktreeGroup> {
    let criteria = &settings.worktree_sort;
    let repo_names: HashMap<String, String> = repos.iter().map(|r| (r.id.clone(), r.name.clone())).collect();

    let mut by_repo: HashMap<&str, Vec<WorktreeView>> = repos.iter().map(|r| (r.id.as_str(), Vec::new())).collect();
    for v in views {
        if let Some(bucket) = by_repo.get_mut(v.worktree.repo_id.as_str()) {
            bucket.push(v);
        }
    }

    let mut ordered: Vec<&Repository> = repos.iter().collect();
    if let Some(rc) = criteria.iter().find(|c| c.enabled && c.key == SortKey::Repo) {
        ordered.sort_by(|a, b| {
            let r = locale_cmp(&a.name, &b.name);
            match rc.direction {
                Direction::Asc => r,
                Direction::Desc => r.reverse(),
            }
        });
    }

    let single = repos.len() == 1;
    let mut groups = Vec::new();
    for repo in ordered {
        let mut worktrees = by_repo.remove(repo.id.as_str()).unwrap_or_default();
        // Stable, like JS Array.prototype.sort.
        worktrees.sort_by(|a, b| compare(a, b, criteria, &repo_names));
        if !worktrees.is_empty() || single {
            groups.push(WorktreeGroup { repo: repo.clone(), worktrees });
        }
    }
    groups
}

// ---------------------------------------------------------------------------
// Tests (ported from tests/lib/grouping.test.ts and tests/lib/sorting.test.ts)
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::model::{GitStatus, LinearInfo, LinearProject, LinearState, PrInfo, Worktree};

    // ---- fixtures ----

    pub(crate) fn repo(id: &str, name: &str) -> Repository {
        Repository { id: id.into(), path: format!("/tmp/{id}"), name: name.into() }
    }

    /// A `WorktreeView` fixture with the same defaults as the TS `makeWorktree`
    /// helper; chain the builder methods to override fields.
    pub(crate) fn wt(id: &str) -> WorktreeView {
        WorktreeView {
            worktree: Worktree {
                id: id.into(),
                repo_id: "repo-1".into(),
                path: "/tmp/wt".into(),
                branch: "feature/test".into(),
                name: "test".into(),
                custom_name: None,
                is_main: false,
                created_at: "2024-01-01".into(),
            },
            agent: None,
            git: None,
            last_commit: None,
            pr: None,
            linear: None,
            has_terminal: false,
            open_ide: None,
            running_processes: Vec::new(),
        }
    }

    pub(crate) trait ViewExt {
        fn repo_id(self, r: &str) -> Self;
        fn branch(self, b: &str) -> Self;
        fn main(self) -> Self;
        fn created(self, at: &str) -> Self;
        fn agent(self, s: Status, updated_at: &str, is_open: bool) -> Self;
        fn linear(self, ident: &str, project: Option<&str>) -> Self;
        fn pr(self, f: impl FnOnce(&mut PrInfo)) -> Self;
        fn dirty(self, n: u32) -> Self;
    }

    impl ViewExt for WorktreeView {
        fn repo_id(mut self, r: &str) -> Self {
            self.worktree.repo_id = r.into();
            self
        }
        fn branch(mut self, b: &str) -> Self {
            self.worktree.branch = b.into();
            self
        }
        fn main(mut self) -> Self {
            self.worktree.is_main = true;
            self
        }
        fn created(mut self, at: &str) -> Self {
            self.worktree.created_at = at.into();
            self
        }
        fn agent(mut self, status: Status, updated_at: &str, is_open: bool) -> Self {
            self.agent = Some(AgentState {
                status,
                session_id: None,
                last_response: None,
                is_open,
                active_subagents: 0,
                updated_at: updated_at.into(),
            });
            self
        }
        fn linear(mut self, ident: &str, project: Option<&str>) -> Self {
            self.linear = Some(LinearInfo {
                identifier: ident.into(),
                title: format!("Ticket {ident}"),
                url: format!("https://linear.app/team/issue/{ident}"),
                state: LinearState { name: "In Progress".into(), color: "#0ea5e9".into(), kind: "started".into() },
                priority_label: "High".into(),
                assignee: None,
                project: project.map(|name| LinearProject {
                    id: format!("proj-{}", name.to_lowercase()),
                    name: name.into(),
                    color: None,
                    url: None,
                }),
                pr_attachment: None,
            });
            self
        }
        fn pr(mut self, f: impl FnOnce(&mut PrInfo)) -> Self {
            let mut pr = PrInfo {
                number: 1,
                title: "PR".into(),
                url: "https://example.com/pr/1".into(),
                state: "OPEN".into(),
                is_draft: false,
                review_decision: String::new(),
                has_feedback: false,
                checks_status: ChecksStatus::None,
                active_check_url: None,
                active_check_name: None,
                checks_waiting: false,
            };
            f(&mut pr);
            self.pr = Some(pr);
            self
        }
        fn dirty(mut self, n: u32) -> Self {
            self.git = Some(GitStatus { ahead: 0, behind: 0, dirty: n });
            self
        }
    }

    fn only(key: SortKey, direction: Direction) -> Vec<SortCriterion> {
        vec![SortCriterion { key, direction, enabled: true }]
    }

    fn sorted(mut v: Vec<WorktreeView>, criteria: &[SortCriterion]) -> Vec<String> {
        let names = HashMap::new();
        v.sort_by(|a, b| compare(a, b, criteria, &names));
        ids(&v)
    }

    fn ids(v: &[WorktreeView]) -> Vec<String> {
        v.iter().map(|w| w.worktree.id.clone()).collect()
    }

    fn flat(groups: &[WorktreeGroup]) -> Vec<WorktreeView> {
        groups.iter().flat_map(|g| g.worktrees.clone()).collect()
    }

    fn with_sort(criteria: Vec<SortCriterion>) -> Settings {
        Settings { worktree_sort: criteria, ..Settings::default() }
    }

    fn no_filters() -> Settings {
        Settings {
            hide_main_branch: false,
            hide_merged_closed_prs: false,
            hide_idle_done_agents: false,
            hide_without_linear_ticket: false,
            ..Settings::default()
        }
    }

    fn filtered(v: Vec<WorktreeView>, s: &Settings) -> Vec<String> {
        // Fixed "now" so the idle-grace rule is deterministic.
        ids(&apply_filters_with_cutoff(v, s, "2024-01-01 00:00:00"))
    }

    use Direction::{Asc, Desc};

    // ---- compareWorktrees (grouping.test.ts) / makeComparator (sorting.test.ts) ----

    #[test]
    fn default_sort_puts_dedicated_before_main_clusters_tickets_newest_first() {
        let v = vec![
            wt("main").main().created("2024-01-05"),
            wt("plain").created("2024-01-04"),
            wt("eng1a").linear("ENG-1", None).created("2024-01-01"),
            wt("eng1b").linear("ENG-1", None).created("2024-01-03"),
        ];
        assert_eq!(sorted(v, &Settings::default().worktree_sort), ["eng1b", "eng1a", "plain", "main"]);
    }

    #[test]
    fn agent_status_active_first() {
        let v = vec![
            wt("none"),
            wt("idle").agent(Status::Idle, "2024-01-01", false),
            wt("exec").agent(Status::Executing, "2024-01-01", false),
        ];
        assert_eq!(sorted(v, &only(SortKey::AgentStatus, Asc)), ["exec", "idle", "none"]);
    }

    #[test]
    fn agent_status_full_rank_order_treats_delegating_as_active() {
        let v = vec![
            wt("none"),
            wt("delegating").agent(Status::Delegating, "x", false),
            wt("done").agent(Status::Done, "x", false),
            wt("idle").agent(Status::Idle, "x", false),
            wt("waiting").agent(Status::Waiting, "x", false),
            wt("planning").agent(Status::Planning, "x", false),
            wt("executing").agent(Status::Executing, "x", false),
        ];
        assert_eq!(
            sorted(v, &only(SortKey::AgentStatus, Asc)),
            ["executing", "delegating", "planning", "waiting", "idle", "done", "none"]
        );
    }

    #[test]
    fn pr_status_attention_first() {
        let v = vec![
            wt("no-pr"),
            wt("merged").pr(|p| p.state = "MERGED".into()),
            wt("open").pr(|p| p.state = "OPEN".into()),
            wt("failing").pr(|p| p.checks_status = ChecksStatus::Failing),
        ];
        assert_eq!(sorted(v, &only(SortKey::PrStatus, Asc)), ["failing", "open", "merged", "no-pr"]);
    }

    #[test]
    fn pr_status_full_rank_order() {
        let v = vec![
            wt("none"),
            wt("closed").pr(|p| p.state = "CLOSED".into()),
            wt("merged").pr(|p| p.state = "MERGED".into()),
            wt("draft").pr(|p| p.is_draft = true),
            wt("open").pr(|_| {}),
            wt("pending").pr(|p| p.checks_status = ChecksStatus::Pending),
            wt("changes").pr(|p| p.review_decision = "CHANGES_REQUESTED".into()),
            wt("failing").pr(|p| p.checks_status = ChecksStatus::Failing),
        ];
        assert_eq!(
            sorted(v, &only(SortKey::PrStatus, Asc)),
            ["failing", "changes", "pending", "open", "draft", "merged", "closed", "none"]
        );
    }

    #[test]
    fn last_activity_desc_puts_missing_status_last() {
        let v = vec![
            wt("none"),
            wt("older").agent(Status::Idle, "2024-01-01", false),
            wt("recent").agent(Status::Idle, "2024-03-01", false),
        ];
        assert_eq!(sorted(v, &only(SortKey::LastActivity, Desc)), ["recent", "older", "none"]);
    }

    #[test]
    fn branches_alphabetical() {
        let v = vec![wt("b").branch("feature/b"), wt("a").branch("feature/a")];
        assert_eq!(sorted(v, &only(SortKey::BranchName, Asc)), ["a", "b"]);
    }

    #[test]
    fn branch_names_compare_case_insensitively_like_locale_compare() {
        let v = vec![wt("B").branch("Beta"), wt("a").branch("alpha"), wt("c").branch("charlie")];
        assert_eq!(sorted(v, &only(SortKey::BranchName, Asc)), ["a", "B", "c"]);
    }

    #[test]
    fn dirty_first() {
        let v = vec![wt("clean").dirty(0), wt("dirty").dirty(3)];
        assert_eq!(sorted(v, &only(SortKey::GitDirty, Asc)), ["dirty", "clean"]);
    }

    #[test]
    fn projectless_last_for_linear_project() {
        let v = vec![wt("without"), wt("with").linear("ENG-1", Some("Alpha"))];
        assert_eq!(sorted(v, &only(SortKey::LinearProject, Asc)), ["with", "without"]);
    }

    #[test]
    fn ticketless_last_for_linear_ticket_even_when_desc() {
        // present_first is negated by Desc like every other key, matching TS.
        let v = vec![wt("none"), wt("a").linear("ENG-1", None), wt("b").linear("ENG-2", None)];
        assert_eq!(sorted(v.clone(), &only(SortKey::LinearTicket, Asc)), ["a", "b", "none"]);
        assert_eq!(sorted(v, &only(SortKey::LinearTicket, Desc)), ["none", "b", "a"]);
    }

    #[test]
    fn desc_negates() {
        let a = wt("a").branch("aaa");
        let b = wt("b").branch("bbb");
        assert_eq!(sorted(vec![b.clone(), a.clone()], &only(SortKey::BranchName, Asc)), ["a", "b"]);
        assert_eq!(sorted(vec![a, b], &only(SortKey::BranchName, Desc)), ["b", "a"]);
    }

    #[test]
    fn criteria_in_order_first_non_zero_wins() {
        let v = vec![wt("main").main().branch("aaa"), wt("ded").branch("zzz")];
        let criteria = [
            SortCriterion { key: SortKey::IsMain, direction: Asc, enabled: true },
            SortCriterion { key: SortKey::BranchName, direction: Asc, enabled: true },
        ];
        assert_eq!(sorted(v, &criteria), ["ded", "main"]);
    }

    #[test]
    fn disabled_criteria_skipped() {
        let v = vec![wt("b").branch("bbb").created("2024-01-01"), wt("a").branch("aaa").created("2024-01-02")];
        let criteria = [
            SortCriterion { key: SortKey::BranchName, direction: Asc, enabled: false },
            SortCriterion { key: SortKey::CreatedAt, direction: Desc, enabled: true },
        ];
        assert_eq!(sorted(v, &criteria), ["a", "b"]);
    }

    #[test]
    fn all_tied_is_equal() {
        let names = HashMap::new();
        assert_eq!(compare(&wt("a"), &wt("b"), &only(SortKey::IsMain, Asc), &names), Ordering::Equal);
    }

    #[test]
    fn repo_key_compares_repo_names_in_a_flat_list() {
        let names: HashMap<String, String> =
            [("r1".to_string(), "web".to_string()), ("r2".to_string(), "api".to_string())].into();
        let a = wt("a").repo_id("r1");
        let b = wt("b").repo_id("r2");
        assert_eq!(compare(&a, &b, &only(SortKey::Repo, Asc), &names), Ordering::Greater);
        assert_eq!(compare(&a, &a.clone(), &only(SortKey::Repo, Asc), &names), Ordering::Equal);
    }

    #[test]
    fn top_sort_key_is_first_enabled() {
        assert_eq!(top_sort_key(&Settings::default()), Some(SortKey::IsMain));
        let s = with_sort(vec![
            SortCriterion { key: SortKey::IsMain, direction: Asc, enabled: false },
            SortCriterion { key: SortKey::LinearProject, direction: Asc, enabled: true },
        ]);
        assert_eq!(top_sort_key(&s), Some(SortKey::LinearProject));
        assert_eq!(top_sort_key(&with_sort(vec![])), None);
    }

    // ---- applyWorktreeFilters ----

    #[test]
    fn hides_main_branch() {
        let s = Settings { hide_main_branch: true, ..no_filters() };
        let v = vec![wt("main").main().branch("main"), wt("ded").branch("feature/x")];
        assert_eq!(filtered(v, &s), ["ded"]);
    }

    #[test]
    fn hides_master_but_not_a_main_checkout_on_another_branch() {
        let s = Settings { hide_main_branch: true, ..no_filters() };
        let v = vec![wt("master").main().branch("master"), wt("other").main().branch("develop")];
        assert_eq!(filtered(v, &s), ["other"]);
    }

    #[test]
    fn keeps_main_branch_with_effectively_open_session() {
        let s = Settings { hide_main_branch: true, ..no_filters() };
        let v = vec![wt("main").main().branch("main").agent(Status::Executing, "2024-01-01", true)];
        assert_eq!(filtered(v, &s), ["main"]);
    }

    #[test]
    fn stale_idle_open_session_is_not_effectively_open() {
        let s = Settings { hide_main_branch: true, ..no_filters() };
        // cutoff (now - 10min) is 2024-01-01 00:00:00
        let v = vec![
            wt("stale").main().branch("main").agent(Status::Idle, "2023-12-31 23:59:59", true),
            wt("fresh").main().branch("main").agent(Status::Idle, "2024-01-01 00:00:01", true),
            wt("closed").main().branch("main").agent(Status::Executing, "2024-01-01 00:00:01", false),
        ];
        assert_eq!(filtered(v, &s), ["fresh"]);
    }

    #[test]
    fn hides_merged_and_closed_prs() {
        let s = Settings { hide_merged_closed_prs: true, ..no_filters() };
        let v = vec![
            wt("merged").pr(|p| p.state = "MERGED".into()),
            wt("closed").pr(|p| p.state = "CLOSED".into()),
            wt("open").pr(|p| p.state = "OPEN".into()),
        ];
        assert_eq!(filtered(v, &s), ["open"]);
    }

    #[test]
    fn hides_idle_done_and_statusless_agents() {
        let s = Settings { hide_idle_done_agents: true, ..no_filters() };
        let v = vec![
            wt("idle").agent(Status::Idle, "2024-01-01", false),
            wt("done").agent(Status::Done, "2024-01-01", false),
            wt("none"),
            wt("exec").agent(Status::Executing, "2024-01-01", false),
            wt("delegating").agent(Status::Delegating, "2024-01-01", false),
        ];
        assert_eq!(filtered(v, &s), ["exec", "delegating"]);
    }

    #[test]
    fn hides_without_linear_ticket() {
        let s = Settings { hide_without_linear_ticket: true, ..no_filters() };
        let v = vec![wt("with").linear("ENG-1", None), wt("without")];
        assert_eq!(filtered(v, &s), ["with"]);
    }

    #[test]
    fn no_filters_keeps_everything() {
        let v = vec![wt("a").main().branch("main"), wt("b")];
        assert_eq!(filtered(v, &no_filters()), ["a", "b"]);
    }

    #[test]
    fn default_settings_hide_the_plain_main_branch() {
        let v = vec![wt("main").main().branch("main")];
        assert!(filtered(v.clone(), &Settings::default()).is_empty());
        // The public entry point (real clock) agrees.
        assert!(apply_filters(v, &Settings::default()).is_empty());
    }

    // ---- buildGroups ----

    #[test]
    fn one_group_per_repo_sorted_by_criteria() {
        let repos = [repo("repo-1", "test-repo")];
        let v = vec![wt("wt-2").created("2024-01-01"), wt("wt-1").created("2024-01-02")];
        let groups = build_groups(&repos, v, &Settings::default());
        assert_eq!(groups.len(), 1);
        assert_eq!(ids(&groups[0].worktrees), ["wt-1", "wt-2"]);
        assert_eq!(ids(&flat(&groups)), ["wt-1", "wt-2"]);
    }

    #[test]
    fn keeps_empty_group_for_single_repo() {
        let groups = build_groups(&[repo("repo-1", "test-repo")], vec![], &Settings::default());
        assert_eq!(groups.len(), 1);
        assert!(groups[0].worktrees.is_empty());
    }

    #[test]
    fn drops_empty_groups_with_multiple_repos() {
        let repos = [repo("r1", "test-repo"), repo("r2", "test-repo")];
        let groups = build_groups(&repos, vec![wt("wt-1").repo_id("r2")], &Settings::default());
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].repo.id, "r2");
    }

    #[test]
    fn preserves_repo_config_order() {
        let repos = [repo("r1", "repo-one"), repo("r2", "repo-two")];
        let v = vec![wt("b").repo_id("r2"), wt("a").repo_id("r1")];
        let groups = build_groups(&repos, v, &Settings::default());
        assert_eq!(groups.iter().map(|g| g.repo.id.as_str()).collect::<Vec<_>>(), ["r1", "r2"]);
    }

    #[test]
    fn orders_repo_sections_by_name_when_repo_criterion_enabled() {
        let repos = [repo("r-web", "web-app"), repo("r-api", "agent-monitor")];
        let v = vec![wt("w").repo_id("r-web"), wt("a").repo_id("r-api")];
        let names = |g: &[WorktreeGroup]| g.iter().map(|g| g.repo.name.clone()).collect::<Vec<_>>();

        for (dir, expected) in [(Asc, ["agent-monitor", "web-app"]), (Desc, ["web-app", "agent-monitor"])] {
            let mut sort = vec![SortCriterion { key: SortKey::Repo, direction: dir, enabled: true }];
            sort.extend(Settings::default().worktree_sort);
            let groups = build_groups(&repos, v.clone(), &with_sort(sort));
            assert_eq!(names(&groups), expected);
        }

        // Without an enabled repo criterion, config order wins (the default
        // list contains a disabled `repo` entry).
        let groups = build_groups(&repos, v, &Settings::default());
        assert_eq!(names(&groups), ["web-app", "agent-monitor"]);
    }

    #[test]
    fn clusters_shared_linear_ticket_adjacently() {
        let repos = [repo("repo-1", "test-repo")];
        let v = vec![
            wt("other").linear("ENG-2", None).created("2024-01-03"),
            wt("a").linear("ENG-1", Some("Alpha")).created("2024-01-01"),
            wt("b").linear("ENG-1", Some("Alpha")).created("2024-01-02"),
        ];
        let groups = build_groups(&repos, v, &Settings::default());
        assert_eq!(ids(&flat(&groups)), ["b", "a", "other"]);
    }

    #[test]
    fn clusters_by_linear_project_when_it_leads() {
        let repos = [repo("repo-1", "test-repo")];
        let v = vec![
            wt("b").linear("BUG-1", Some("Beta")),
            wt("a").linear("ENG-9", Some("Alpha")),
            wt("a2").linear("ENG-1", Some("Alpha")),
        ];
        let groups = build_groups(&repos, v, &with_sort(only(SortKey::LinearProject, Asc)));
        let projects: Vec<_> = flat(&groups)
            .iter()
            .map(|w| w.linear.as_ref().unwrap().project.as_ref().unwrap().name.clone())
            .collect();
        assert_eq!(projects, ["Alpha", "Alpha", "Beta"]);
    }

    #[test]
    fn flat_order_equals_concatenated_groups() {
        let repos = [repo("r1", "test-repo"), repo("r2", "test-repo")];
        let v = vec![wt("w1").repo_id("r1"), wt("w2").repo_id("r1").main(), wt("w3").repo_id("r2")];
        let groups = build_groups(&repos, v, &Settings::default());
        let all = flat(&groups);
        assert_eq!(all.len(), 3);
        assert_eq!(ids(&all), ["w1", "w2", "w3"]);
    }

    #[test]
    fn views_for_unknown_repos_are_dropped() {
        let repos = [repo("r1", "a"), repo("r2", "b")];
        let v = vec![wt("w1").repo_id("r1"), wt("ghost").repo_id("gone")];
        let groups = build_groups(&repos, v, &Settings::default());
        assert_eq!(ids(&flat(&groups)), ["w1"]);
    }

    #[test]
    fn locale_cmp_orders_like_icu_for_common_identifiers() {
        assert_eq!(locale_cmp("a", "B"), Ordering::Less);
        assert_eq!(locale_cmp("a", "A"), Ordering::Less);
        assert_eq!(locale_cmp("feature/a", "feature/b"), Ordering::Less);
        assert_eq!(locale_cmp("x-1", "x1"), Ordering::Less);
        assert_eq!(locale_cmp("2024-01-02 00:00:00", "2024-01-01 23:59:59"), Ordering::Greater);
        assert_eq!(locale_cmp("", "a"), Ordering::Less);
    }
}
