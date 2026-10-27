// Performance regression tests over a large fixture (many worktrees, review
// requests and review groups).
//
// - **Scaling** (always on): the same operation at a small and an 8× larger
//   fixture. Linear work grows ~8×, quadratic ~64×; the bound sits between,
//   so this catches an O(n²) slip without depending on how fast the machine
//   is. Each point is the best of several runs.
// - **Budgets** (`#[ignore]`, run in release): absolute per-call timings,
//   printed as a table:
//   `cargo test --release -p am-tui perf_budget -- --ignored --nocapture`
//
// Declared inside `ui::tests`, so it uses that module's `view`/`render` helpers.

use std::time::{Duration, Instant};

use super::*;
use crate::reviews_ui::tests::review;
use am_core::review_groups::ReviewGroup;

const REPOS: [&str; 4] = ["agent-monitor", "gridsight", "gridsight-fork", "todo-widget"];

/// `worktrees` rows over 4 repos (PRs, tickets shared in pairs, stacks, a
/// three-session row every 7th, subagents) and `reviews` review requests
/// (stacked, some ticketed, several authors) with three badge groups.
pub(super) fn big_fixture(worktrees: usize, reviews: usize) -> App {
    let repos: Vec<Repository> = REPOS
        .iter()
        .enumerate()
        .map(|(i, name)| Repository { id: format!("r{i}"), path: format!("/{name}"), name: (*name).into() })
        .collect();
    let mut groups: Vec<WorktreeGroup> = repos.iter().map(|r| WorktreeGroup { repo: r.clone(), worktrees: vec![] }).collect();
    let mut stack = std::collections::HashMap::new();
    let mut subagents = Vec::new();
    let statuses = [Some(Status::Executing), Some(Status::Waiting), Some(Status::Done), Some(Status::Idle), None];
    for i in 0..worktrees {
        let r = i % REPOS.len();
        let id = format!("w{i}");
        let mut v = view(&id, &format!("r{r}"), &format!("feature/eng-{i}-some-work"), statuses[i % statuses.len()]);
        if i % 3 == 0 {
            v.pr = Some(PrInfo {
                number: 100 + i as u64,
                title: format!("Work {i}"),
                url: "u".into(),
                state: "OPEN".into(),
                is_draft: i % 6 == 0,
                review_decision: "REVIEW_REQUIRED".into(),
                has_feedback: false,
                checks_status: ChecksStatus::Passing,
                active_check_url: None,
                active_check_name: None,
                checks_waiting: false,
                base_ref_name: None,
                mergeable: None,
            });
        }
        if i % 2 == 0 {
            v.linear = Some(LinearInfo {
                identifier: format!("ENG-{}", i / 4),
                title: format!("Ticket {}", i / 4),
                url: "u".into(),
                state: LinearState { name: "In Progress".into(), color: String::new(), kind: "started".into() },
                priority_label: String::new(),
                assignee: None,
                project: None,
                pr_attachment: None,
            });
        }
        v.context = Some(ContextUsage { tokens: 10_000 * (i as u64 % 90), model: None });
        if i % 7 == 0 {
            v.sessions = (0..3)
                .map(|k| SessionView {
                    agent: Default::default(),
                    session_id: format!("{k}a7c-{i}"),
                    status: Status::Executing,
                    last_response: None,
                    started_at: "2025-10-11 09:00:00".into(),
                    updated_at: "2025-10-11 09:59:00".into(),
                    context: None,
                })
                .collect();
        }
        if i % 5 == 4 {
            // Stacked on the previous row of the same repo.
            stack.insert(id.clone(), format!("w{}", i - REPOS.len()));
        }
        if i % 4 == 0 {
            subagents.push(sub(&id, &format!("a{i}"), "Research", true, Some(("Bash", "ls"))));
        }
        groups[r].worktrees.push(v);
    }
    let reviews: Vec<ReviewRequest> = (0..reviews)
        .map(|i| {
            let r = i % REPOS.len();
            let mut rv = review(&format!("r{r}"), 1_000 + i as u64, &format!("2026-10-{:02}T10:00:00Z", 1 + i % 26));
            rv.author = ["alice", "bob", "carol", "dave", "erin"][i % 5].into();
            rv.created_at = format!("2026-09-{:02}T10:00:00Z", 1 + i % 28);
            rv.pr.title = format!("Feeder forecast part {i}");
            if i % 4 == 3 {
                // Stacked on the review before it in the same repo.
                rv.pr.base_ref_name = Some(format!("feature/{}", 1_000 + i - REPOS.len()));
            }
            if i % 3 == 0 {
                rv.requested = vec!["acme/platform".into()];
            }
            rv
        })
        .collect();
    let snapshot = Snapshot {
        repos,
        groups,
        subagents,
        health: Default::default(),
        stack,
        today_tokens: None,
        reviews: Some(reviews),
        review_error: None,
        review_people: vec![],
        review_total: Some(500),
        review_query_hits: Default::default(),
    };
    let group = |name: &str, f: &dyn Fn(&mut ReviewGroup)| {
        let mut g = ReviewGroup { name: name.into(), ..ReviewGroup::default() };
        f(&mut g);
        g
    };
    let settings = Settings {
        worktree_sort: vec![],
        review_badge_total: true,
        review_groups: vec![
            group("Carol", &|g| g.authors = vec!["carol".into()]),
            group("Forecasts", &|g| g.text = "forecast part 1".into()),
            group("Stale", &|g| {
                g.older_than_days = 20;
                g.sort = "oldest".into();
            }),
        ],
        ..Settings::default()
    };
    let mut app = App::new(settings);
    app.apply(snapshot);
    app
}

/// Best of `runs` timings of `reps` calls of `f`, per call.
fn best(runs: usize, reps: usize, mut f: impl FnMut()) -> Duration {
    (0..runs)
        .map(|_| {
            let started = Instant::now();
            for _ in 0..reps {
                f();
            }
            started.elapsed() / reps as u32
        })
        .min()
        .unwrap()
}

/// The operations under test, each timed on a fresh fixture of `(worktrees, reviews)`.
fn measure(n: (usize, usize), reps: usize) -> Vec<(&'static str, Duration)> {
    let mut out = Vec::new();
    let app = big_fixture(n.0, n.1);
    out.push(("dashboard frame", best(5, reps, || drop(render(&app, 160, 60)))));
    let snap = app.snapshot.clone();
    let mut app = big_fixture(n.0, n.1);
    out.push(("apply snapshot", best(5, reps, || app.apply(snap.clone()))));
    out.push((
        "key + frame, dashboard",
        best(5, reps, || {
            app.on_key(Key::Char('j'));
            drop(render(&app, 160, 60));
        }),
    ));
    let mut page = big_fixture(n.0, n.1);
    page.reviews = Some(crate::reviews_ui::ReviewPage::default());
    out.push(("review page frame", best(5, reps, || drop(render(&page, 160, 60)))));
    out.push((
        "key + frame, review page",
        best(5, reps, || {
            page.on_key(Key::Char('j'));
            drop(render(&page, 160, 60));
        }),
    ));
    out
}

const SMALL: (usize, usize) = (50, 40);
const LARGE: (usize, usize) = (400, 320);
/// Between linear (8×) and quadratic (64×) growth for an 8× larger fixture.
const MAX_GROWTH: f64 = 24.0;

#[test]
fn work_grows_no_worse_than_linearly_with_worktrees_and_reviews() {
    // Alternate the sizes and keep each one's best, so a burst of load from
    // tests running alongside doesn't land on one side only.
    let min = |a: Vec<(&'static str, Duration)>, b: Vec<(&'static str, Duration)>| {
        a.into_iter().zip(b).map(|((n, x), (_, y))| (n, x.min(y))).collect::<Vec<_>>()
    };
    let (small, large) = (measure(SMALL, 3), measure(LARGE, 3));
    let (small, large) = (min(small, measure(SMALL, 3)), min(large, measure(LARGE, 3)));
    let mut report = String::new();
    let mut failed = Vec::new();
    for ((name, s), (_, l)) in small.iter().zip(&large) {
        // A floor keeps a sub-10µs operation's timer noise from deciding it.
        let growth = l.as_secs_f64() / s.as_secs_f64().max(10e-6);
        report.push_str(&format!("{name:<20} {s:>12?} → {l:>12?}  ×{growth:.1}\n"));
        if growth > MAX_GROWTH {
            failed.push(*name);
        }
    }
    assert!(failed.is_empty(), "grew faster than ×{MAX_GROWTH} for an 8× larger fixture: {failed:?}\n{report}");
}

/// Per-call budgets for a release build at roughly the size of a busy setup
/// (and 4× that). Generous: they flag a step change, not a few percent.
#[test]
#[ignore = "timing budgets: cargo test --release -p am-tui perf_budget -- --ignored --nocapture"]
fn perf_budget() {
    let budgets: [(&str, Duration); 5] = [
        ("dashboard frame", Duration::from_millis(4)),
        ("apply snapshot", Duration::from_millis(4)),
        ("key + frame, dashboard", Duration::from_millis(4)),
        ("review page frame", Duration::from_millis(4)),
        ("key + frame, review page", Duration::from_millis(4)),
    ];
    let mut failed = Vec::new();
    for size in [(60, 50), (240, 200)] {
        println!("\n{} worktrees, {} reviews", size.0, size.1);
        for (name, took) in measure(size, 10) {
            let budget = budgets.iter().find(|(b, _)| *b == name).map(|(_, d)| *d).unwrap();
            let budget = if size.0 > 60 { budget * 4 } else { budget };
            let ok = took <= budget;
            println!("  {name:<20} {took:>12?}  budget {budget:>8?}  {}", if ok { "ok" } else { "SLOW" });
            if !ok {
                failed.push(format!("{name} at {size:?}: {took:?} > {budget:?}"));
            }
        }
    }
    assert!(failed.is_empty(), "over budget: {failed:?}");
}
