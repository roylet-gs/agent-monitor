//! Manual, read-only probe of the integrations against real (or mocked) services:
//!
//! ```sh
//! cargo run -p am-core --example integrations_probe -- <repo-path> <branch>...
//! ```
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
    println!("health: {:?}", ints.health());
    let _ = std::fs::remove_dir_all(&dir);
}
