//! Manual, read-only probe of the integrations against real (or mocked) services:
//!
//! ```sh
//! cargo run -p am-core --example integrations_probe -- <repo-path> <branch>...
//! cargo run -p am-core --example integrations_probe -- --tick <repo-path> <branch>...
//! ```
//!
//! `--tick` runs the TUI's event-driven refresh instead (git + REST detector +
//! targeted GraphQL): a first sweep, then quiet ticks, printing every `gh`
//! call it makes and the PR fields shown (git facts included).
//!
//! Uses a throwaway in-memory cache DB, so the real `integration_cache` is
//! never touched. The Linear key is read from settings and never printed;
//! Linear is skipped unless `linearEnabled` is true.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use am_core::integrations::github::{GhOutput, GhRunner, SystemGh};
use am_core::integrations::gitpr::SystemGit;
use am_core::integrations::linear::UreqTransport;
use am_core::integrations::{GithubTick, Integrations, RepoBranches};

/// Real gh that logs each call.
#[derive(Clone, Default)]
struct LoggingGh(Arc<Mutex<Vec<String>>>);

impl GhRunner for LoggingGh {
    fn run(&self, cwd: &Path, args: &[String], timeout: Duration) -> GhOutput {
        let label = if args.get(1).map(String::as_str) == Some("graphql") {
            "api graphql".to_string()
        } else {
            args.iter().filter(|a| !a.starts_with("If-None-Match") && *a != "-H").cloned().collect::<Vec<_>>().join(" ")
        };
        let out = SystemGh.run(cwd, args, timeout);
        let status = out.stdout.lines().next().filter(|l| l.starts_with("HTTP/")).unwrap_or("").to_string();
        self.0.lock().unwrap().push(format!("{label} {status}"));
        out
    }
}

fn tick_probe(repo: String, branches: Vec<String>) {
    let dir = std::env::temp_dir().join(format!("am-probe-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let conn = am_core::db::open_at(&dir.join("probe.db")).unwrap();
    let repos = vec![RepoBranches::new("probe", &repo, branches.clone())];
    let gh = LoggingGh::default();
    let mut ints = Integrations::with_git(
        Box::new(gh.clone()),
        Box::new(SystemGit),
        Box::new(UreqTransport::default()),
        am_core::integrations::linear::linear_api_url(),
    );
    for (i, sweep) in [true, false, false].into_iter().enumerate() {
        let t = std::time::Instant::now();
        let out = ints.tick_github(
            &conn,
            GithubTick { repos: &repos, force: false, sweep, interval: Duration::from_secs(180), hints: &[] },
        );
        println!("tick {i} (sweep={sweep}): {out:?} in {:?}", t.elapsed());
        for call in gh.0.lock().unwrap().drain(..) {
            println!("    gh {call}");
        }
        if i == 0 {
            for b in &branches {
                let pr = ints.fields_for("probe", b).0;
                let desc = pr.map(|p| format!("#{} {} mergeable={:?} title={:?}", p.number, p.state, p.mergeable, p.title));
                println!("  {b}: {}", desc.unwrap_or_else(|| "no PR".into()));
            }
        }
        std::thread::sleep(Duration::from_secs(21));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

fn main() {
    let mut args = std::env::args().skip(1).peekable();
    if args.peek().map(String::as_str) == Some("--tick") {
        args.next();
        let repo = args.next().expect("usage: integrations_probe --tick <repo-path> <branch>...");
        return tick_probe(repo, args.collect());
    }
    let repo = args.next().expect("usage: integrations_probe <repo-path> <branch>...");
    let branches: Vec<String> = args.collect();

    let dir = std::env::temp_dir().join(format!("am-probe-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let conn = am_core::db::open_at(&dir.join("probe.db")).unwrap();
    let repos = vec![RepoBranches::new("probe", &repo, branches.clone())];
    let mut ints = Integrations::new();

    let t = std::time::Instant::now();
    let outcome = ints.refresh_github(&conn, &repos, true);
    println!("github: {outcome:?} in {:?}", t.elapsed());
    for b in &branches {
        let pr = ints.cached_pr("probe", b);
        println!("  {b}: {}", pr.map(|p| serde_json::to_string(p).unwrap()).unwrap_or_else(|| "no PR".into()));
    }

    let settings = am_core::settings::load();
    if settings.linear_enabled && !settings.linear_api_key.is_empty() {
        let t = std::time::Instant::now();
        let outcome = ints.refresh_linear(&conn, &settings.linear_api_key, &repos, true);
        println!("linear: {outcome:?} in {:?}", t.elapsed());
        for b in &branches {
            let li = ints.cached_linear("probe", b);
            let desc = li.map(|l| {
                let project = l.project.as_ref().map(|p| p.name.as_str()).unwrap_or("-");
                format!("{} [{}] project={project} pr_attachment={}", l.identifier, l.state.name, l.pr_attachment.is_some())
            });
            println!("  {b}: {}", desc.unwrap_or_else(|| "no ticket".into()));
        }
    } else {
        println!("linear: skipped (linearEnabled is false or no key)");
    }
    println!("health: {:?}", ints.health());
    let _ = std::fs::remove_dir_all(&dir);
}
