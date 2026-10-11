//! `am-hook` — the native replacement for `am hook-event`.
//!
//!   am-hook hook-event --worktree <path> [--event <name>] [--managed]
//!
//! The handling lives in the library ([`am_hook::run_hook_event`]) so that
//! `am-rs hook-event` runs exactly the same code.

fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("hook-event") => {}
        Some("--version") | Some("-V") => {
            println!("am-hook {}", env!("CARGO_PKG_VERSION"));
            std::process::exit(0);
        }
        _ => {
            eprintln!("{}", am_hook::USAGE);
            std::process::exit(2);
        }
    }
    match am_hook::parse_hook_args(args) {
        Ok(a) => am_hook::run_hook_event(&a.worktree, a.event.as_deref()),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    }
    // Hooks must never block or fail Claude Code: always exit 0.
}
