//! The review-requests page (`v`): open PRs waiting on your review, in the
//! `reviewSort` order. Pure state; `App` turns its keys into actions.

use std::cmp::Ordering;

use am_core::model::{Repository, ReviewRequest};
use am_core::settings::ReviewSort;

/// A visual row of the page. Headers take no cursor index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewRow {
    /// Section header: a repo (`repo` order) or who was asked (`direct`),
    /// shown when there's more than one section.
    Header(String),
    /// Index into `ReviewPage::order`'s list.
    Pr(usize),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReviewPage {
    pub cursor: usize,
}

/// The review asks you by name (not only one of your teams).
pub fn asks_you(r: &ReviewRequest) -> bool {
    r.requested.iter().any(|w| w == "you")
}

/// Which section a PR falls in under `sort`, as its header text.
fn section(r: &ReviewRequest, sort: ReviewSort, repos: &[Repository]) -> Option<String> {
    match sort {
        ReviewSort::Direct => Some(if asks_you(r) { "Asked you".into() } else { "Asked your team".into() }),
        ReviewSort::Repo => {
            Some(repos.iter().find(|x| x.id == r.repo_id).map_or(r.repo.clone(), |x| x.name.clone()))
        }
        ReviewSort::Updated | ReviewSort::Oldest => None,
    }
}

/// The list in page order (`reviewSort`). Returns indices into `reviews`.
pub fn order(reviews: &[ReviewRequest], repos: &[Repository], sort: ReviewSort) -> Vec<usize> {
    let rank = |id: &str| repos.iter().position(|r| r.id == id).unwrap_or(usize::MAX);
    let newest = |a: &ReviewRequest, b: &ReviewRequest| b.updated_at.cmp(&a.updated_at);
    let mut idx: Vec<usize> = (0..reviews.len()).collect();
    idx.sort_by(|&a, &b| {
        let (a, b) = (&reviews[a], &reviews[b]);
        let first: Ordering = match sort {
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
pub fn rows(reviews: &[ReviewRequest], ordered: &[usize], repos: &[Repository], sort: ReviewSort) -> Vec<ReviewRow> {
    let sections: Vec<Option<String>> = ordered.iter().map(|&i| section(&reviews[i], sort, repos)).collect();
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
        if len == 0 {
            self.cursor = 0;
            return;
        }
        self.cursor = (self.cursor as isize).saturating_add(delta).clamp(0, len as isize - 1) as usize;
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
            worktree_id: None,
        }
    }

    fn repo(id: &str) -> Repository {
        Repository { id: id.into(), path: format!("/{id}"), name: id.into() }
    }

    #[test]
    fn grouped_by_repo_then_newest_first() {
        let list = vec![review("b", 1, "2026-10-01"), review("a", 2, "2026-10-01"), review("b", 3, "2026-10-05")];
        let repos = vec![repo("a"), repo("b")];
        let ordered = order(&list, &repos, ReviewSort::Repo);
        assert_eq!(ordered, [1, 2, 0]);
        assert_eq!(
            rows(&list, &ordered, &repos, ReviewSort::Repo),
            [ReviewRow::Header("a".into()), ReviewRow::Pr(0), ReviewRow::Header("b".into()), ReviewRow::Pr(1), ReviewRow::Pr(2)]
        );
        // One repo: no header.
        let one = vec![review("a", 1, "x")];
        assert_eq!(rows(&one, &[0], &repos, ReviewSort::Repo), [ReviewRow::Pr(0)]);
    }

    #[test]
    fn asked_you_by_name_comes_first_by_default() {
        let list = vec![
            review_asking("a", 1, "2026-10-09", "acme/platform"),
            review_asking("b", 2, "2026-10-01", "you"),
            review_asking("a", 3, "2026-10-05", "you"),
        ];
        let repos = vec![repo("a"), repo("b")];
        let ordered = order(&list, &repos, ReviewSort::Direct);
        assert_eq!(ordered, [2, 1, 0]);
        assert_eq!(
            rows(&list, &ordered, &repos, ReviewSort::Direct),
            [
                ReviewRow::Header("Asked you".into()),
                ReviewRow::Pr(0),
                ReviewRow::Pr(1),
                ReviewRow::Header("Asked your team".into()),
                ReviewRow::Pr(2)
            ]
        );
        // Flat orders have no headers.
        assert_eq!(order(&list, &repos, ReviewSort::Updated), [0, 2, 1]);
        let mut aged = list.clone();
        aged[0].created_at = "2026-09-01".into();
        aged[1].created_at = "2026-09-20".into();
        aged[2].created_at = "2026-09-10".into();
        assert_eq!(order(&aged, &repos, ReviewSort::Oldest), [0, 2, 1]);
        assert!(rows(&list, &[0, 2, 1], &repos, ReviewSort::Oldest).iter().all(|r| matches!(r, ReviewRow::Pr(_))));
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
