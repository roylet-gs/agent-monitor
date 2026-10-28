//! The review-requests page (`v`): open PRs waiting on your review, split
//! into the review groups (`reviewGroups`, then `reviewDefaultGroup`; see
//! `am_core::review_groups`), each in its own `sort` order.
//! Pure state; `App` turns its keys into actions.

use std::cmp::Ordering;
#[cfg(test)]
use std::sync::LazyLock;

use am_core::model::{Repository, ReviewRequest};
use am_core::review_groups::{QueryHits, ReviewGroup, assign};
use am_core::settings::{ReviewSort, Settings};

/// A visual row of the page. Headers take no cursor index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewRow {
    /// Section header: a group's name, or (in the unnamed default group) a
    /// repo (`repo` order) or who was asked (`direct`); shown when there's
    /// more than one section.
    Header(String),
    /// Index into `ReviewPage::order`'s list.
    Pr(usize),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReviewPage {
    pub cursor: usize,
    /// `V`: beside the worktree list instead of in its place.
    pub split: bool,
    /// In a split, whether the page (not the list) takes the keys; a full
    /// page always does.
    pub focused: bool,
    /// First visible row (`ui::scroll_start`).
    pub scroll: std::cell::Cell<usize>,
}

/// The review groups the page is split by, and the time their age rules
/// are measured from.
#[derive(Debug, Clone, Copy)]
pub struct Grouping<'a> {
    pub groups: &'a [ReviewGroup],
    pub default: &'a ReviewGroup,
    pub now: i64,
    /// Which PRs match each group's GitHub search terms.
    pub hits: &'a QueryHits,
}

#[cfg(test)]
static NO_HITS: LazyLock<QueryHits> = LazyLock::new(QueryHits::new);

impl<'a> Grouping<'a> {
    pub fn new(settings: &'a Settings, now: i64, hits: &'a QueryHits) -> Self {
        Grouping { groups: &settings.review_groups, default: &settings.review_default_group, now, hits }
    }

    /// The order group `g` (`groups.len()` = the default) sorts by.
    pub fn sort(&self, g: usize) -> ReviewSort {
        let group = self.groups.get(g).unwrap_or(self.default);
        ReviewSort::parse(&group.sort).unwrap_or(ReviewSort::Direct)
    }

    /// The page is one list in repo order: rows needn't name their repo.
    pub fn by_repo(&self) -> bool {
        self.groups.is_empty() && self.sort(0) == ReviewSort::Repo
    }


    /// Header of group `g` (`groups.len()` = the default group), when it has
    /// one of its own.
    fn name(&self, g: usize) -> Option<String> {
        match self.groups.get(g) {
            Some(group) if group.name.trim().is_empty() => Some(format!("Group {}", g + 1)),
            Some(group) => Some(group.name.trim().to_string()),
            None => Some(self.default.name.trim().to_string()).filter(|n| !n.is_empty()),
        }
    }
}

/// The review asks you by name (not only one of your teams).
pub fn asks_you(r: &ReviewRequest) -> bool {
    r.requested.iter().any(|w| w == "you")
}

/// The group each of `reviews` is shown in (`None` = hidden): the first
/// whose rules it matches, except that a PR stacked on a shown PR follows
/// it, so a stack is never split.
pub fn groups_of(reviews: &[ReviewRequest], repos: &[Repository], g: Grouping) -> Vec<Option<usize>> {
    let mut out = assign(reviews, g.groups, g.default, repos, g.now, g.hits);
    let all: Vec<usize> = (0..reviews.len()).collect();
    let parents: Vec<Option<usize>> = reviews.iter().map(|r| stacked_on(reviews, &all, r)).collect();
    // A chain settles in at most its length.
    for _ in 0..reviews.len() {
        let mut changed = false;
        for (i, parent) in parents.iter().enumerate() {
            if let Some(p) = *parent
                && p != i
                && out[p].is_some()
                && out[i] != out[p]
            {
                out[i] = out[p];
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    out
}

/// How many PRs each group shows (the default group last) and how many are
/// hidden.
pub fn counts(reviews: &[ReviewRequest], repos: &[Repository], g: Grouping) -> (Vec<usize>, usize) {
    let mut per = vec![0; g.groups.len() + 1];
    let mut hidden = 0;
    for grp in groups_of(reviews, repos, g) {
        match grp {
            Some(i) => per[i] += 1,
            None => hidden += 1,
        }
    }
    (per, hidden)
}

/// Which section a PR of group `grp` falls in, as its header text.
fn section(r: &ReviewRequest, grp: usize, g: Grouping, repos: &[Repository]) -> Option<String> {
    if let Some(name) = g.name(grp) {
        return Some(name);
    }
    match g.sort(grp) {
        ReviewSort::Direct => Some(if asks_you(r) { "Asked you".into() } else { "Asked your team".into() }),
        ReviewSort::Repo => {
            Some(repos.iter().find(|x| x.id == r.repo_id).map_or(r.repo.clone(), |x| x.name.clone()))
        }
        ReviewSort::Updated | ReviewSort::Oldest if !g.groups.is_empty() => Some("Everything else".into()),
        ReviewSort::Updated | ReviewSort::Oldest => None,
    }
}

/// The PR among `list` (indices into `reviews`) whose head branch `r`'s
/// PR targets, in the same repo: `r` is stacked on it.
fn stacked_on(reviews: &[ReviewRequest], list: &[usize], r: &ReviewRequest) -> Option<usize> {
    let base = r.pr.base_ref_name.as_deref()?;
    list.iter().position(|&j| {
        let p = &reviews[j];
        !p.is_cross_repository && p.repo_id == r.repo_id && p.head_ref_name == base
    })
}

/// Tree prefix per entry of `ordered` (from [`order`]): "" for a PR on its
/// own, "├─ "/"└─ "… for one stacked on another listed PR.
pub fn prefixes(reviews: &[ReviewRequest], ordered: &[usize]) -> Vec<String> {
    let tree = crate::app::tree_order(ordered.len(), |k| stacked_on(reviews, ordered, &reviews[ordered[k]]));
    let mut out = vec![String::new(); ordered.len()];
    for (k, prefix) in tree {
        out[k] = prefix;
    }
    out
}

/// The shown PRs in page order (group by group, each in its own order),
/// with stacked PRs right under the PR they target. Returns indices into
/// `reviews`.
pub fn order(reviews: &[ReviewRequest], repos: &[Repository], g: Grouping) -> Vec<usize> {
    let groups = groups_of(reviews, repos, g);
    let sorted = sorted(reviews, &groups, repos, g);
    crate::app::tree_order(sorted.len(), |k| stacked_on(reviews, &sorted, &reviews[sorted[k]]))
        .into_iter()
        .map(|(k, _)| sorted[k])
        .collect()
}

fn sorted(reviews: &[ReviewRequest], groups: &[Option<usize>], repos: &[Repository], g: Grouping) -> Vec<usize> {
    let rank = |id: &str| repos.iter().position(|r| r.id == id).unwrap_or(usize::MAX);
    let newest = |a: &ReviewRequest, b: &ReviewRequest| b.updated_at.cmp(&a.updated_at);
    let mut idx: Vec<usize> = (0..reviews.len()).filter(|&i| groups[i].is_some()).collect();
    idx.sort_by(|&ia, &ib| {
        let (a, b) = (&reviews[ia], &reviews[ib]);
        if groups[ia] != groups[ib] {
            return groups[ia].cmp(&groups[ib]);
        }
        let first: Ordering = match g.sort(groups[ia].unwrap_or(0)) {
            ReviewSort::Direct => asks_you(b).cmp(&asks_you(a)),
            ReviewSort::Repo => rank(&a.repo_id).cmp(&rank(&b.repo_id)),
            ReviewSort::Updated => Ordering::Equal,
            ReviewSort::Oldest => a.created_at.cmp(&b.created_at),
        };
        first.then_with(|| newest(a, b))
    });
    idx
}

/// Rows for `ordered` (indices from [`order`]): a header before each
/// section when there's more than one.
pub fn rows(
    reviews: &[ReviewRequest],
    ordered: &[usize],
    repos: &[Repository],
    g: Grouping,
) -> Vec<ReviewRow> {
    let groups = groups_of(reviews, repos, g);
    // A stacked PR stays in its parent's section.
    let mut sections: Vec<Option<String>> = Vec::with_capacity(ordered.len());
    for (k, prefix) in prefixes(reviews, ordered).into_iter().enumerate() {
        let i = ordered[k];
        let sec = match sections.last() {
            Some(prev) if !prefix.is_empty() => prev.clone(),
            _ => section(&reviews[i], groups[i].unwrap_or(g.groups.len()), g, repos),
        };
        sections.push(sec);
    }
    let several = sections.windows(2).any(|w| w[0] != w[1]);
    let mut out = Vec::new();
    for (pos, sec) in sections.iter().enumerate() {
        if several && (pos == 0 || sections[pos - 1] != *sec) {
            out.extend(sec.clone().map(ReviewRow::Header));
        }
        out.push(ReviewRow::Pr(pos));
    }
    out
}

impl ReviewPage {
    pub fn move_by(&mut self, delta: isize, len: usize) {
        self.cursor = crate::nav::moved(self.cursor, delta, len);
    }

    /// Keep the cursor on the list after it shrinks.
    pub fn clamp(&mut self, len: usize) {
        self.cursor = self.cursor.min(len.saturating_sub(1));
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use am_core::model::{ChecksStatus, PrInfo};

    pub fn review(repo_id: &str, number: u64, updated: &str) -> ReviewRequest {
        review_asking(repo_id, number, updated, "you")
    }

    pub fn review_asking(repo_id: &str, number: u64, updated: &str, who: &str) -> ReviewRequest {
        ReviewRequest {
            repo_id: repo_id.into(),
            repo: format!("acme/{repo_id}"),
            pr: PrInfo {
                number,
                title: format!("PR {number}"),
                url: format!("https://github.com/acme/{repo_id}/pull/{number}"),
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
            author: "alice".into(),
            head_ref_name: format!("feature/{number}"),
            is_cross_repository: false,
            created_at: String::new(),
            updated_at: updated.into(),
            requested: vec![who.into()],
            linear: None,
            reviewed_by_you: false,
            worktree_id: None,
        }
    }

    /// No groups; the default (catching all) in `sort` order.
    fn plain(sort: ReviewSort) -> ReviewGroup {
        ReviewGroup { sort: sort.id().into(), ..ReviewGroup::default() }
    }

    fn by(default: &ReviewGroup) -> Grouping<'_> {
        Grouping { groups: &[], default, now: 0, hits: &NO_HITS }
    }

    fn repo(id: &str) -> Repository {
        Repository { id: id.into(), path: format!("/{id}"), name: id.into() }
    }

    #[test]
    fn grouped_by_repo_then_newest_first() {
        let list = vec![review("b", 1, "2026-10-01"), review("a", 2, "2026-10-01"), review("b", 3, "2026-10-05")];
        let repos = vec![repo("a"), repo("b")];
        let ordered = order(&list, &repos, by(&plain(ReviewSort::Repo)));
        assert_eq!(ordered, [1, 2, 0]);
        assert_eq!(
            rows(&list, &ordered, &repos, by(&plain(ReviewSort::Repo))),
            [ReviewRow::Header("a".into()), ReviewRow::Pr(0), ReviewRow::Header("b".into()), ReviewRow::Pr(1), ReviewRow::Pr(2)]
        );
        // One repo: no header.
        let one = vec![review("a", 1, "x")];
        assert_eq!(rows(&one, &[0], &repos, by(&plain(ReviewSort::Repo))), [ReviewRow::Pr(0)]);
    }

    #[test]
    fn asked_you_by_name_comes_first_by_default() {
        let list = vec![
            review_asking("a", 1, "2026-10-09", "acme/platform"),
            review_asking("b", 2, "2026-10-01", "you"),
            review_asking("a", 3, "2026-10-05", "you"),
        ];
        let repos = vec![repo("a"), repo("b")];
        let ordered = order(&list, &repos, by(&plain(ReviewSort::Direct)));
        assert_eq!(ordered, [2, 1, 0]);
        assert_eq!(
            rows(&list, &ordered, &repos, by(&plain(ReviewSort::Direct))),
            [
                ReviewRow::Header("Asked you".into()),
                ReviewRow::Pr(0),
                ReviewRow::Pr(1),
                ReviewRow::Header("Asked your team".into()),
                ReviewRow::Pr(2)
            ]
        );
        // Flat orders have no headers.
        assert_eq!(order(&list, &repos, by(&plain(ReviewSort::Updated))), [0, 2, 1]);
        let mut aged = list.clone();
        aged[0].created_at = "2026-09-01".into();
        aged[1].created_at = "2026-09-20".into();
        aged[2].created_at = "2026-09-10".into();
        assert_eq!(order(&aged, &repos, by(&plain(ReviewSort::Oldest))), [0, 2, 1]);
        assert!(rows(&list, &[0, 2, 1], &repos, by(&plain(ReviewSort::Oldest))).iter().all(|r| matches!(r, ReviewRow::Pr(_))));
    }

    #[test]
    fn stacked_prs_sit_under_the_pr_they_target() {
        // #2 targets #1's head and #3 targets #2's; #4 targets main. #2
        // asks your team, yet stays under #1 in "Asked you".
        let mut list = vec![
            review("a", 1, "2026-10-01"),
            review_asking("a", 2, "2026-10-09", "acme/platform"),
            review("a", 3, "2026-10-08"),
            review("a", 4, "2026-10-05"),
            review("b", 5, "2026-10-07"),
        ];
        list[1].pr.base_ref_name = Some("feature/1".into());
        list[2].pr.base_ref_name = Some("feature/2".into());
        // Same branch name in another repo: not a stack.
        list[4].pr.base_ref_name = Some("feature/1".into());
        let repos = vec![repo("a"), repo("b")];
        // #3 is the newest asking you, but it follows its stack: #1 → #2 → #3.
        let ordered = order(&list, &repos, by(&plain(ReviewSort::Direct)));
        assert_eq!(ordered, [4, 3, 0, 1, 2]);
        assert_eq!(prefixes(&list, &ordered), ["", "", "", "└─ ", "   └─ "]);
        let rows = rows(&list, &ordered, &repos, by(&plain(ReviewSort::Direct)));
        assert!(!rows.contains(&ReviewRow::Header("Asked your team".into())), "{rows:?}");
        // A sibling stack branches.
        list[3].pr.base_ref_name = Some("feature/1".into());
        let ordered = order(&list, &repos, by(&plain(ReviewSort::Updated)));
        assert_eq!(ordered, [4, 0, 1, 2, 3]);
        assert_eq!(prefixes(&list, &ordered), ["", "", "├─ ", "│  └─ ", "└─ "]);
    }

    fn groups(json: &str) -> Vec<ReviewGroup> {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn groups_cascade_and_the_default_hides_what_it_does_not_match() {
        let mut list = vec![
            review_asking("a", 1, "2026-10-09", "you"),
            review_asking("b", 2, "2026-10-01", "acme/platform"),
            review_asking("a", 3, "2026-10-05", "you"),
            review("a", 4, "2026-10-03"),
        ];
        list[1].author = "Bob".into();
        let repos = vec![repo("a"), repo("b")];
        let gs = groups(r#"[{"name": "Bob", "authors": ["bob"]}, {"asked": "you", "repos": ["b"]}]"#);
        let all = ReviewGroup::default();
        let g = Grouping { groups: &gs, default: &all, now: 0, hits: &NO_HITS };
        let header = |s: &str| ReviewRow::Header(s.into());

        // Bob's PR first; the second group (unnamed) catches nothing; the
        // unnamed default keeps the "Asked you" sections.
        let ordered = order(&list, &repos, g);
        assert_eq!(ordered, [1, 0, 2, 3]);
        assert_eq!(
            rows(&list, &ordered, &repos, g),
            [header("Bob"), ReviewRow::Pr(0), header("Asked you"), ReviewRow::Pr(1), ReviewRow::Pr(2), ReviewRow::Pr(3)]
        );
        assert_eq!(counts(&list, &repos, g), (vec![1, 0, 3], 0));
        // Flat orders: the rest is "Everything else"; a named default says its name.
        let upd = plain(ReviewSort::Updated);
        let gu = Grouping { default: &upd, ..g };
        let r = rows(&list, &order(&list, &repos, gu), &repos, gu);
        assert_eq!(r[2], header("Everything else"));
        let named = groups(r#"[{"name": "Rest", "olderThanDays": 0}]"#).remove(0);
        let g2 = Grouping { default: &named, ..g };
        assert_eq!(rows(&list, &order(&list, &repos, g2), &repos, g2)[2], header("Rest"));
        // An unnamed group reads "Group N".
        list[1].repo_id = "b".into();
        list[1].requested = vec!["you".into()];
        list[1].author = "dan".into();
        let r = rows(&list, &order(&list, &repos, g), &repos, g);
        assert_eq!(r[0], header("Group 2"));

        // A default that only takes PRs asking you hides the rest.
        let only_you = groups(r#"[{"asked": "you"}]"#).remove(0);
        list[3].requested = vec!["acme/platform".into()];
        let g3 = Grouping { groups: &[], default: &only_you, now: 0, hits: &NO_HITS };
        assert_eq!(order(&list, &repos, g3), [0, 2, 1]);
        assert_eq!(counts(&list, &repos, g3), (vec![3], 1));
    }

    #[test]
    fn each_group_has_its_own_order_and_github_terms() {
        let mut list = vec![review("a", 1, "2026-10-01"), review("a", 2, "2026-10-09"), review("a", 3, "2026-10-05")];
        list[0].created_at = "2026-09-01".into();
        list[1].created_at = "2026-09-20".into();
        list[2].created_at = "2026-09-10".into();
        list[2].pr.url = "u3".into();
        let gs = groups(r#"[{"name": "Urgent", "query": "label:urgent"}, {"name": "Old first", "sort": "oldest"}]"#);
        let hits = QueryHits::from([("label:urgent".to_string(), vec!["u3".to_string()])]);
        let all = ReviewGroup::default();
        let g = Grouping { groups: &gs, default: &all, now: 0, hits: &hits };
        // #3 matches GitHub's search for label:urgent; the rest go oldest first.
        assert_eq!(order(&list, &[repo("a")], g), [2, 0, 1]);
        let none = QueryHits::new();
        assert_eq!(order(&list, &[repo("a")], Grouping { hits: &none, ..g }), [0, 2, 1], "terms not fetched: no match");
    }

    #[test]
    fn a_stacked_pr_follows_its_parent_into_its_group() {
        let mut list = vec![review("a", 1, "2026-10-01"), review("a", 2, "2026-10-09"), review("a", 3, "2026-10-08")];
        list[0].author = "bob".into();
        list[1].pr.base_ref_name = Some("feature/1".into());
        list[2].pr.base_ref_name = Some("feature/2".into());
        let gs = groups(r#"[{"name": "Bob", "authors": ["bob"]}]"#);
        // The default would hide #2 and #3 — but they're stacked on Bob's #1.
        let none = groups(r#"[{"authors": ["nobody"]}]"#).remove(0);
        let g = Grouping { groups: &gs, default: &none, now: 0, hits: &NO_HITS };
        assert_eq!(groups_of(&list, &[repo("a")], g), [Some(0), Some(0), Some(0)]);
        assert_eq!(order(&list, &[repo("a")], g), [0, 1, 2]);
    }

    #[test]
    fn cursor_stays_in_range() {
        let mut p = ReviewPage::default();
        p.move_by(5, 3);
        assert_eq!(p.cursor, 2);
        p.move_by(-9, 3);
        assert_eq!(p.cursor, 0);
        p.cursor = 4;
        p.clamp(2);
        assert_eq!(p.cursor, 1);
    }
}
