//! Review groups: the review page's sections as a cascade of filters
//! (`reviewGroups`, then `reviewDefaultGroup`).
//!
//! Each PR goes to the first group whose rules it matches; what no group
//! catches falls to the default group, and what the default group's rules
//! don't match either is hidden. Within a group every rule must match; a
//! rule's list (authors, teams, repos) matches any of its entries. An unset
//! rule (empty list, `any`, 0, "") matches everything, so a group without
//! rules catches all that reaches it. A group's `query` (GitHub search
//! terms) can't be checked locally: the review search runs it alongside the
//! list and hands back the matching PRs ([`QueryHits`]). Pure; shared by the
//! TUI and tests.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::model::{ChecksStatus, Repository, ReviewRequest};

/// One group of the review page. Every field is optional in the file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ReviewGroup {
    /// Section header (and badge label). The default group without a name
    /// keeps its `sort`'s sections (`Asked you`…).
    pub name: String,
    /// Order within the group: `direct` (asking you by name first), `repo`,
    /// `updated` or `oldest`.
    pub sort: String,
    /// GitHub logins of PR authors (`@` optional).
    pub authors: Vec<String>,
    /// Who the review is asked of: `any`, `you` (by name) or `team` (one of
    /// your teams).
    pub asked: String,
    /// Requested teams, `team` or `org/team`.
    pub teams: Vec<String>,
    /// Tracked repos: directory name, `owner/name` or GitHub name.
    pub repos: Vec<String>,
    /// Never these tracked repos (same format).
    pub exclude_repos: Vec<String>,
    /// `any`, `draft` or `ready`.
    pub draft: String,
    /// `any`, `passing`, `failing` or `pending`.
    pub checks: String,
    /// Review decision: `any`, `approved`, `changes` or `required`.
    pub review: String,
    /// You've already reviewed it: `any`, `yes` or `no`.
    pub reviewed: String,
    /// Opened more than this many days ago (0 = any age).
    pub older_than_days: u64,
    /// Linear ticket: `any`, `yes` or `no`.
    pub ticket: String,
    /// Case-insensitive text in the title or head branch.
    pub text: String,
    /// GitHub search qualifiers the PR must match (`label:urgent`), checked
    /// by GitHub (see [`QueryHits`]).
    pub query: String,
    /// Show this group's count in the dashboard's review badge.
    pub badge: bool,
    /// Off, the group takes nothing: its PRs fall through to the next one
    /// (the default group off hides everything left).
    pub enabled: bool,
}

impl Default for ReviewGroup {
    fn default() -> Self {
        ReviewGroup {
            name: String::new(),
            sort: "direct".into(),
            authors: Vec::new(),
            asked: "any".into(),
            teams: Vec::new(),
            repos: Vec::new(),
            exclude_repos: Vec::new(),
            draft: "any".into(),
            checks: "any".into(),
            review: "any".into(),
            reviewed: "any".into(),
            older_than_days: 0,
            ticket: "any".into(),
            text: String::new(),
            query: String::new(),
            badge: true,
            enabled: true,
        }
    }
}

pub const ASKED: [&str; 3] = ["any", "you", "team"];
pub const DRAFT: [&str; 3] = ["any", "draft", "ready"];
pub const CHECKS: [&str; 4] = ["any", "passing", "failing", "pending"];
pub const REVIEW: [&str; 4] = ["any", "approved", "changes", "required"];
pub const TICKET: [&str; 3] = ["any", "yes", "no"];
pub const REVIEWED: [&str; 3] = ["any", "yes", "no"];

/// PR urls matching each group's `query`, keyed by [`query_key`]; filled by
/// the review search. A query not in it (not fetched yet) matches nothing.
pub type QueryHits = HashMap<String, Vec<String>>;

/// A group query as searched and looked up: words joined by one space.
pub fn query_key(query: &str) -> String {
    query.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Every distinct non-empty query among `groups` and `default`.
pub fn queries(groups: &[ReviewGroup], default: &ReviewGroup) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for q in groups.iter().chain([default]).map(|g| query_key(&g.query)).filter(|q| !q.is_empty()) {
        if !out.contains(&q) {
            out.push(q);
        }
    }
    out
}

/// A login or team as compared: lower-case, no `@`.
fn norm(s: &str) -> String {
    s.trim().trim_start_matches('@').to_lowercase()
}

fn any_of(list: &[String], hit: impl Fn(&str) -> bool) -> bool {
    let list: Vec<String> = list.iter().map(|s| norm(s)).filter(|s| !s.is_empty()).collect();
    list.is_empty() || list.iter().any(|s| hit(s))
}

impl ReviewGroup {
    /// Whether `r` passes every rule. `repo_name` is its tracked repo's
    /// directory name; `now` is unix seconds (for `olderThanDays`).
    pub fn matches(&self, r: &ReviewRequest, repo_name: &str, now: i64, hits: &QueryHits) -> bool {
        let author = r.author.to_lowercase();
        let slug = r.repo.to_lowercase();
        let short = slug.rsplit('/').next().unwrap_or("").to_string();
        let asks_you = r.requested.iter().any(|w| w == "you");
        let teams: Vec<String> = r.requested.iter().filter(|w| w.contains('/')).map(|w| w.to_lowercase()).collect();
        let hay = format!("{}\n{}", r.pr.title, r.head_ref_name).to_lowercase();
        any_of(&self.authors, |a| a == author)
            && match self.asked.as_str() {
                "you" => asks_you,
                "team" => !teams.is_empty(),
                _ => true,
            }
            && any_of(&self.teams, |t| teams.iter().any(|x| *x == t || x.rsplit('/').next() == Some(t)))
            && any_of(&self.repos, |x| x == repo_name.to_lowercase() || x == slug || x == short)
            && !self.exclude_repos.iter().map(|x| norm(x)).any(|x| x == repo_name.to_lowercase() || x == slug || x == short)
            && match self.draft.as_str() {
                "draft" => r.pr.is_draft,
                "ready" => !r.pr.is_draft,
                _ => true,
            }
            && match self.checks.as_str() {
                "passing" => r.pr.checks_status == ChecksStatus::Passing,
                "failing" => r.pr.checks_status == ChecksStatus::Failing,
                "pending" => r.pr.checks_status == ChecksStatus::Pending,
                _ => true,
            }
            && match self.review.as_str() {
                "approved" => r.pr.review_decision == "APPROVED",
                "changes" => r.pr.review_decision == "CHANGES_REQUESTED",
                "required" => r.pr.review_decision == "REVIEW_REQUIRED",
                _ => true,
            }
            && match self.reviewed.as_str() {
                "yes" => r.reviewed_by_you,
                "no" => !r.reviewed_by_you,
                _ => true,
            }
            && (self.older_than_days == 0
                || crate::timefmt::parse_iso(&r.created_at)
                    .is_some_and(|t| now - t > self.older_than_days as i64 * 86_400))
            && match self.ticket.as_str() {
                "yes" => r.linear.is_some(),
                "no" => r.linear.is_none(),
                _ => true,
            }
            && (self.text.trim().is_empty() || hay.contains(&self.text.trim().to_lowercase()))
            && match query_key(&self.query).as_str() {
                "" => true,
                q => hits.get(q).is_some_and(|urls| urls.contains(&r.pr.url)),
            }
    }

    /// The rules in a few words, for the groups page (`""` = catches all).
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        let list = |v: &[String], at: bool| {
            v.iter().map(|s| norm(s)).filter(|s| !s.is_empty()).map(|s| if at { format!("@{s}") } else { s }).collect::<Vec<_>>()
        };
        let authors = list(&self.authors, true);
        if !authors.is_empty() {
            parts.push(format!("by {}", authors.join(", ")));
        }
        match self.asked.as_str() {
            "you" => parts.push("asks you".into()),
            "team" => parts.push("asks a team".into()),
            _ => {}
        }
        let teams = list(&self.teams, true);
        if !teams.is_empty() {
            parts.push(format!("team {}", teams.join(", ")));
        }
        let repos = list(&self.repos, false);
        if !repos.is_empty() {
            parts.push(format!("in {}", repos.join(", ")));
        }
        let not = list(&self.exclude_repos, false);
        if !not.is_empty() {
            parts.push(format!("not {}", not.join(", ")));
        }
        match self.draft.as_str() {
            "draft" => parts.push("drafts only".into()),
            "ready" => parts.push("no drafts".into()),
            _ => {}
        }
        match self.checks.as_str() {
            "passing" | "failing" | "pending" => parts.push(format!("CI {}", self.checks)),
            _ => {}
        }
        match self.review.as_str() {
            "approved" => parts.push("approved".into()),
            "changes" => parts.push("changes requested".into()),
            "required" => parts.push("needs review".into()),
            _ => {}
        }
        if self.older_than_days > 0 {
            parts.push(format!("> {}d old", self.older_than_days));
        }
        match self.reviewed.as_str() {
            "yes" => parts.push("reviewed".into()),
            "no" => parts.push("not yet reviewed".into()),
            _ => {}
        }
        match self.ticket.as_str() {
            "yes" => parts.push("has ticket".into()),
            "no" => parts.push("no ticket".into()),
            _ => {}
        }
        if !self.text.trim().is_empty() {
            parts.push(format!("“{}”", self.text.trim()));
        }
        if !query_key(&self.query).is_empty() {
            parts.push(query_key(&self.query));
        }
        parts.join(" · ")
    }
}

/// Which group each of `reviews` lands in: `Some(i)` for `groups[i]`,
/// `Some(groups.len())` for the default group, `None` when hidden.
pub fn assign(
    reviews: &[ReviewRequest],
    groups: &[ReviewGroup],
    default: &ReviewGroup,
    repos: &[Repository],
    now: i64,
    hits: &QueryHits,
) -> Vec<Option<usize>> {
    reviews
        .iter()
        .map(|r| {
            let name = repos.iter().find(|x| x.id == r.repo_id).map_or("", |x| x.name.as_str());
            groups
                .iter()
                .position(|g| g.enabled && g.matches(r, name, now, hits))
                .or_else(|| (default.enabled && default.matches(r, name, now, hits)).then_some(groups.len()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{LinearInfo, LinearState, PrInfo};

    fn review(number: u64, author: &str, requested: &[&str]) -> ReviewRequest {
        ReviewRequest {
            repo_id: "r1".into(),
            repo: "acme/web".into(),
            pr: PrInfo {
                number,
                title: format!("Fix thing {number}"),
                url: String::new(),
                state: "OPEN".into(),
                is_draft: false,
                review_decision: "REVIEW_REQUIRED".into(),
                has_feedback: false,
                checks_status: ChecksStatus::Passing,
                active_check_url: None,
                active_check_name: None,
                checks_waiting: false,
                base_ref_name: Some("main".into()),
                mergeable: None,
            },
            author: author.into(),
            head_ref_name: format!("feature/{number}"),
            is_cross_repository: false,
            created_at: "2026-10-01T00:00:00Z".into(),
            updated_at: String::new(),
            requested: requested.iter().map(|s| s.to_string()).collect(),
            linear: None,
            reviewed_by_you: false,
            approved_by_you: false,
            approved_only: false,
            worktree_id: None,
        }
    }

    fn group(json: &str) -> ReviewGroup {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn every_rule_must_match_and_lists_match_any_entry() {
        let now = crate::timefmt::parse_iso("2026-10-27T00:00:00Z").unwrap();
        let r = review(1, "Alice", &["you", "acme/platform"]);
        let hits = QueryHits::from([("label:urgent".to_string(), vec!["u1".to_string()])]);
        let mut r = r;
        r.pr.url = "u1".into();
        let m = |json: &str| group(json).matches(&r, "web-checkout", now, &hits);
        assert!(m("{}"), "no rules: catches everything");
        assert!(m(r#"{"authors": ["bob", "@alice"]}"#));
        assert!(!m(r#"{"authors": ["bob"]}"#));
        assert!(m(r#"{"asked": "you"}"#) && m(r#"{"asked": "team"}"#));
        assert!(m(r#"{"teams": ["platform"]}"#) && m(r#"{"teams": ["@acme/platform"]}"#));
        assert!(!m(r#"{"teams": ["docs"]}"#));
        assert!(m(r#"{"repos": ["web-checkout"]}"#) && m(r#"{"repos": ["acme/web"]}"#) && m(r#"{"repos": ["WEB"]}"#));
        assert!(!m(r#"{"repos": ["api"]}"#));
        assert!(m(r#"{"draft": "ready", "checks": "passing", "review": "required"}"#));
        assert!(!m(r#"{"draft": "draft"}"#) && !m(r#"{"checks": "failing"}"#) && !m(r#"{"review": "approved"}"#));
        assert!(m(r#"{"olderThanDays": 20}"#) && !m(r#"{"olderThanDays": 30}"#));
        assert!(m(r#"{"ticket": "no"}"#) && !m(r#"{"ticket": "yes"}"#));
        assert!(m(r#"{"text": "THING"}"#) && m(r#"{"text": "feature/1"}"#) && !m(r#"{"text": "nope"}"#));
        assert!(!m(r#"{"authors": ["alice"], "asked": "team", "repos": ["api"]}"#), "all must match");
        assert!(!m(r#"{"excludeRepos": ["acme/web"]}"#) && m(r#"{"excludeRepos": ["api"]}"#));
        assert!(m(r#"{"reviewed": "no"}"#) && !m(r#"{"reviewed": "yes"}"#));
        // GitHub-side terms: matched through the search's hits.
        assert!(m(r#"{"query": "  label:urgent "}"#));
        assert!(!m(r#"{"query": "label:later"}"#), "not fetched: matches nothing");

        let mut t = r.clone();
        t.requested = vec!["acme/docs".into()];
        t.linear = Some(LinearInfo {
            identifier: "ENG-1".into(),
            title: String::new(),
            url: String::new(),
            state: LinearState { name: String::new(), color: String::new(), kind: String::new() },
            priority_label: String::new(),
            assignee: None,
            project: None,
            pr_attachment: None,
            parent: None,
        });
        assert!(!group(r#"{"asked": "you"}"#).matches(&t, "", now, &hits));
        assert!(group(r#"{"ticket": "yes"}"#).matches(&t, "", now, &hits));
        t.reviewed_by_you = true;
        assert!(group(r#"{"reviewed": "yes"}"#).matches(&t, "", now, &hits));
    }

    #[test]
    fn prs_cascade_to_the_first_matching_group_then_the_default_or_hidden() {
        let list = vec![review(1, "alice", &["you"]), review(2, "bob", &["acme/platform"]), review(3, "carol", &["you"])];
        let groups = vec![group(r#"{"name": "Alice", "authors": ["alice"]}"#), group(r#"{"name": "Mine", "asked": "you"}"#)];
        // #1 matches both: the first group takes it; #3 falls to the second.
        let all = ReviewGroup::default();
        assert_eq!(assign(&list, &groups, &all, &[], 0, &QueryHits::new()), [Some(0), Some(2), Some(1)]);
        // A filtering default hides what it doesn't match.
        let only_you = group(r#"{"asked": "you"}"#);
        assert_eq!(assign(&list, &groups, &only_you, &[], 0, &QueryHits::new()), [Some(0), None, Some(1)]);
        // A group switched off takes nothing: #1 falls to "Mine".
        let mut off = groups.clone();
        off[0].enabled = false;
        assert_eq!(assign(&list, &off, &all, &[], 0, &QueryHits::new()), [Some(1), Some(2), Some(1)]);
        // The default switched off hides what's left.
        let none = ReviewGroup { enabled: false, ..ReviewGroup::default() };
        assert_eq!(assign(&list, &groups, &none, &[], 0, &QueryHits::new()), [Some(0), None, Some(1)]);
        // No groups: everything goes to the default (index 0).
        assert_eq!(assign(&list, &[], &all, &[], 0, &QueryHits::new()), [Some(0), Some(0), Some(0)]);
    }

    #[test]
    fn queries_are_collected_once_each() {
        let gs = vec![group(r#"{"query": "label:a  -author:x"}"#), group("{}"), group(r#"{"query": "label:a -author:x"}"#)];
        assert_eq!(queries(&gs, &group(r#"{"query": "draft:false"}"#)), ["label:a -author:x", "draft:false"]);
    }

    #[test]
    fn summary_reads_the_rules() {
        assert_eq!(ReviewGroup::default().summary(), "");
        let g = group(r#"{"authors": ["Alice", "@bob"], "asked": "you", "repos": ["web"], "draft": "ready", "olderThanDays": 3}"#);
        assert_eq!(g.summary(), "by @alice, @bob · asks you · in web · no drafts · > 3d old");
    }
}
