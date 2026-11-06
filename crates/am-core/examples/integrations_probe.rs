//! Manual, read-only probe of the integrations against real (or mocked) services:
//!
//! ```sh
//! cargo run -p am-core --example integrations_probe -- <repo-path> <branch>...
//! ```
//!
//! With `AM_PROBE_LIVE_SECS=<n>` it then runs the live watchers
//! (notifications + CI watch) for n seconds, printing each nudge, the
//! targeted refetch it causes and the REST/GraphQL rate-limit use.
//!
//! Uses a throwaway in-memory cache DB, so the real `integration_cache` is
//! never touched. The Linear key is read from settings and never printed;
//! Linear is skipped unless `linearEnabled` is true.

use am_core::integrations::{Integrations, RepoBranches};

fn main() {
    let mut args = std::env::args().skip(1);
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
    if let Some(secs) = std::env::var("AM_PROBE_LIVE_SECS").ok().and_then(|v| v.parse::<u64>().ok()) {
        live(&mut ints, &conn, &repos, secs);
    }
    println!("health: {:?}", ints.health());
    let _ = std::fs::remove_dir_all(&dir);
}

/// `used` of the REST `core` and `graphql` buckets (rate_limit itself is free).
fn rate_used() -> String {
    let out = std::process::Command::new("gh")
        .args(["api", "rate_limit", "--jq", "\"core \\(.resources.core.used) graphql \\(.resources.graphql.used)\""])
        .output();
    out.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
}

fn live(ints: &mut Integrations, conn: &rusqlite::Connection, repos: &[RepoBranches], secs: u64) {
    use std::time::{Duration, Instant};
    println!("live: watching for {secs}s (rate used before: {})", rate_used());
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(secs) {
        let now = Instant::now();
        let n = ints.watch_live(repos, true, now);
        if n.polled {
            println!("  +{:>4}s polled in {:?}: {:?} reviews={}", start.elapsed().as_secs(), now.elapsed(), n.branches, n.reviews);
        }
        if !n.branches.is_empty() {
            let branches = n.branches.iter().map(|(_, b)| b.clone()).collect();
            let targeted = vec![RepoBranches::new("probe", &repos[0].repo_path, branches)];
            println!("    refetch: {:?}", ints.refresh_github_branches(conn, &targeted));
            for (_, b) in &n.branches {
                let pr = ints.cached_pr("probe", b);
                println!("    {b}: {:?}", pr.map(|p| (p.number, &p.state, p.checks_status)));
            }
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    println!("live: rate used after: {}", rate_used());
}
