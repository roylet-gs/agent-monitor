//! `am-rs` — the native `am` CLI (becomes `am` at cut-over). Port of
//! `src/cli.tsx` + `src/commands/**`: same command names, aliases, flags,
//! output formats and exit codes. With no subcommand it execs `am-tui`.
//!
//! `hook-event` runs on every Claude Code hook, so it is dispatched before
//! clap or any other initialisation and shares its code with `am-hook`.

mod commands;
mod hooks_installer;
mod output;
mod resolve;
mod store;

use clap::{Args, Parser, Subcommand};

/// Print to stderr and exit 1, like `console.error(...); process.exit(1)`.
pub fn fail(msg: &str) -> ! {
    eprintln!("{msg}");
    std::process::exit(1)
}

/// Like [`fail`] but on stdout (some TS commands used `console.log` before exiting).
pub fn fail_stdout(msg: &str) -> ! {
    println!("{msg}");
    std::process::exit(1)
}

#[derive(Parser)]
#[command(
    name = "am",
    bin_name = "am-rs",
    version,
    about = "Agent Monitor — TUI dashboard for git worktrees & Claude Code agents"
)]
struct Cli {
    /// Launch with log panel open
    #[arg(long)]
    watch: bool,
    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(Args, Clone)]
struct RepoOpt {
    /// Repository path (default: detect from CWD)
    #[arg(long, value_name = "path")]
    repo: Option<String>,
}

#[derive(Args, Clone)]
struct JsonOpt {
    /// Output as JSON
    #[arg(long)]
    json: bool,
}

#[derive(Subcommand)]
enum Cmd {
    /// Manage worktrees
    #[command(subcommand, alias = "wt")]
    Worktree(WorktreeCmd),
    /// Manage Claude sessions per worktree
    #[command(subcommand, alias = "a")]
    Agent(AgentCmd),
    /// Manage tracked repositories
    #[command(subcommand)]
    Repo(RepoCmd),
    /// View and modify settings
    #[command(subcommand)]
    Settings(SettingsCmd),
    /// Manage Claude Code hooks
    #[command(subcommand)]
    Hooks(HooksCmd),
    /// Install Claude hooks (alias for: hooks install)
    InstallHooks,
    /// Remove Claude hooks (alias for: hooks uninstall)
    UninstallHooks,
    /// Show PR info for a worktree branch
    #[command(args_conflicts_with_subcommands = true)]
    Pr(TargetCmd),
    /// Show Linear ticket info for a worktree branch
    #[command(args_conflicts_with_subcommands = true)]
    Linear(TargetCmd),
    /// Manage startup scripts
    #[command(subcommand)]
    Script(ScriptCmd),
    /// Check system health (hooks, gh CLI, DB, etc.)
    Doctor(JsonOpt),
    /// Show recent logs
    Logs(LogsArgs),
    /// Run the setup wizard
    Setup,
    /// Get/set agent status for a worktree
    Status {
        /// Worktree path
        #[arg(short, long, value_name = "path")]
        worktree: String,
        /// Set agent status (idle, executing, planning, waiting, delegating, done)
        #[arg(long, value_name = "status")]
        set: Option<String>,
    },
    /// Receive hook event from stdin (internal)
    HookEvent {
        /// Worktree path
        #[arg(short, long, value_name = "path")]
        worktree: String,
        /// Event name override
        #[arg(short, long, value_name = "name")]
        event: Option<String>,
        /// Hook is managed by Claude Code (accepted for compatibility)
        #[arg(long)]
        managed: bool,
    },
    /// List worktrees (alias for: worktree list)
    Ls(ListArgs),
    /// Create worktree (alias for: worktree create)
    New(CreateArgs),
    /// Open worktree in IDE (alias for: worktree open)
    Open {
        target: String,
        #[command(flatten)]
        repo: RepoOpt,
    },
}

#[derive(Args, Clone)]
struct ListArgs {
    #[command(flatten)]
    repo: RepoOpt,
    #[command(flatten)]
    json: JsonOpt,
}

#[derive(Args, Clone)]
struct CreateArgs {
    branch: String,
    #[command(flatten)]
    repo: RepoOpt,
    /// Custom worktree name
    #[arg(long, value_name = "name")]
    name: Option<String>,
    /// Base branch (default: main)
    #[arg(long, value_name = "branch")]
    base: Option<String>,
    /// Reuse existing local branch
    #[arg(long)]
    reuse: bool,
    /// Pull remote branch (track origin/<branch>)
    #[arg(long)]
    track: bool,
    /// Create new local branch from base, do not track remote
    #[arg(long)]
    no_track: bool,
    #[command(flatten)]
    json: JsonOpt,
}

#[derive(Subcommand)]
enum WorktreeCmd {
    /// List worktrees with status, PR, and Linear info
    List(ListArgs),
    /// Create a new worktree
    Create(CreateArgs),
    /// Delete a worktree
    Delete {
        target: String,
        #[command(flatten)]
        repo: RepoOpt,
        /// Force delete even with uncommitted changes
        #[arg(long)]
        force: bool,
        /// Also delete the local branch
        #[arg(long)]
        delete_branch: bool,
        /// Also delete the remote branch
        #[arg(long)]
        delete_remote: bool,
    },
    /// Open a worktree in IDE
    Open {
        target: String,
        #[command(flatten)]
        repo: RepoOpt,
    },
    /// Sync git worktree state with database
    Sync(RepoOpt),
    /// Show detailed info for a worktree
    Info {
        target: String,
        #[command(flatten)]
        repo: RepoOpt,
        #[command(flatten)]
        json: JsonOpt,
    },
}

#[derive(Subcommand)]
enum AgentCmd {
    /// Resume the worktree's latest Claude session in this terminal (claude --resume)
    Attach {
        target: String,
        #[command(flatten)]
        repo: RepoOpt,
        /// Accepted for compatibility (there are no headless turns to conflict with)
        #[arg(long)]
        force: bool,
    },
}

#[derive(Subcommand)]
enum RepoCmd {
    /// List tracked repositories
    List(JsonOpt),
    /// Add a repository to track
    Add {
        path: String,
        #[command(flatten)]
        json: JsonOpt,
    },
    /// Remove a tracked repository
    Remove {
        #[arg(value_name = "name-or-path")]
        name_or_path: String,
    },
}

#[derive(Subcommand)]
enum SettingsCmd {
    /// Show all settings
    List(JsonOpt),
    /// Get a setting value
    Get {
        key: String,
        #[command(flatten)]
        json: JsonOpt,
    },
    /// Set a setting value
    Set { key: String, value: String },
    /// Reset all settings to defaults
    Reset,
}

#[derive(Subcommand)]
enum HooksCmd {
    /// Install Claude hooks into ~/.claude/settings.json
    Install,
    /// Remove agent-monitor hooks from ~/.claude/settings.json
    Uninstall,
    /// Check if hooks are installed
    Status(JsonOpt),
}

#[derive(Args)]
struct TargetCmd {
    /// Worktree path or branch (default: the current branch)
    target: Option<String>,
    #[command(flatten)]
    repo: RepoOpt,
    #[command(flatten)]
    json: JsonOpt,
    #[command(subcommand)]
    sub: Option<TargetSub>,
}

#[derive(Subcommand)]
enum TargetSub {
    /// Show info (same as without a subcommand)
    Show {
        target: Option<String>,
        #[command(flatten)]
        repo: RepoOpt,
        #[command(flatten)]
        json: JsonOpt,
    },
    /// Open in browser
    Open {
        target: Option<String>,
        #[command(flatten)]
        repo: RepoOpt,
    },
}

#[derive(Subcommand)]
enum ScriptCmd {
    /// Create/open startup script in IDE
    Edit(RepoOpt),
    /// Remove startup script
    Remove(RepoOpt),
    /// Print startup script contents
    Show {
        #[command(flatten)]
        repo: RepoOpt,
        #[command(flatten)]
        json: JsonOpt,
    },
}

#[derive(Args)]
struct LogsArgs {
    /// Number of log lines to show
    #[arg(short = 'n', long, value_name = "number", default_value = "50")]
    lines: String,
    /// Follow log output
    #[arg(short, long)]
    follow: bool,
    /// Filter by level (debug, info, warn, error)
    #[arg(long, value_name = "level")]
    level: Option<String>,
    /// Filter by module
    #[arg(long, value_name = "module")]
    module: Option<String>,
    /// Clear the log file
    #[arg(long)]
    clear: bool,
}

fn main() {
    // Fast path: no clap, no settings, no logger before the hook handler.
    let mut argv = std::env::args();
    if argv.nth(1).as_deref() == Some("hook-event") {
        match am_hook::parse_hook_args(argv) {
            Ok(a) => am_hook::run_hook_event(&a.worktree, a.event.as_deref()),
            Err(e) => fail(&e),
        }
        return;
    }

    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(e) => {
            let _ = e.print();
            // commander exits 1 on usage errors; help/version exit 0.
            std::process::exit(if e.use_stderr() { 1 } else { 0 });
        }
    };

    let s = am_core::settings::load();
    am_core::log::init(s.log_level(), s.max_log_size_mb);

    use commands::*;
    match cli.command {
        None => tui::launch(cli.watch, false),
        Some(Cmd::Setup) => tui::launch(false, true),
        Some(Cmd::Worktree(w)) => match w {
            WorktreeCmd::List(a) => worktree::list(a.repo.repo.as_deref(), a.json.json),
            WorktreeCmd::Create(a) => lifecycle::create(&a),
            WorktreeCmd::Delete { target, repo, force, delete_branch, delete_remote } => {
                lifecycle::delete(&target, repo.repo.as_deref(), force, delete_branch, delete_remote)
            }
            WorktreeCmd::Sync(r) => lifecycle::sync(r.repo.as_deref()),
            WorktreeCmd::Open { target, repo } => worktree::open(&target, repo.repo.as_deref()),
            WorktreeCmd::Info { target, repo, json } => worktree::info(&target, repo.repo.as_deref(), json.json),
        },
        Some(Cmd::Ls(a)) => worktree::list(a.repo.repo.as_deref(), a.json.json),
        Some(Cmd::New(a)) => lifecycle::create(&a),
        Some(Cmd::Open { target, repo }) => worktree::open(&target, repo.repo.as_deref()),
        Some(Cmd::Agent(AgentCmd::Attach { target, repo, force: _ })) => agent::attach(&target, repo.repo.as_deref()),
        Some(Cmd::Repo(r)) => match r {
            RepoCmd::List(j) => repo::list(j.json),
            RepoCmd::Add { path, json } => repo::add(&path, json.json),
            RepoCmd::Remove { name_or_path } => repo::remove(&name_or_path),
        },
        Some(Cmd::Settings(c)) => match c {
            SettingsCmd::List(j) => settings::list(j.json),
            SettingsCmd::Get { key, json } => settings::get(&key, json.json),
            SettingsCmd::Set { key, value } => settings::set(&key, &value),
            SettingsCmd::Reset => settings::reset(),
        },
        Some(Cmd::Hooks(h)) => match h {
            HooksCmd::Install => hooks::install(),
            HooksCmd::Uninstall => hooks::uninstall(),
            HooksCmd::Status(j) => hooks::status(j.json),
        },
        Some(Cmd::InstallHooks) => hooks::install(),
        Some(Cmd::UninstallHooks) => hooks::uninstall(),
        Some(Cmd::Pr(t)) => match t.sub {
            None => pr::show(t.target.as_deref(), t.repo.repo.as_deref(), t.json.json),
            Some(TargetSub::Show { target, repo, json }) => pr::show(target.as_deref(), repo.repo.as_deref(), json.json),
            Some(TargetSub::Open { target, repo }) => pr::open(target.as_deref(), repo.repo.as_deref()),
        },
        Some(Cmd::Linear(t)) => match t.sub {
            None => linear::show(t.target.as_deref(), t.repo.repo.as_deref(), t.json.json),
            Some(TargetSub::Show { target, repo, json }) => {
                linear::show(target.as_deref(), repo.repo.as_deref(), json.json)
            }
            Some(TargetSub::Open { target, repo }) => linear::open(target.as_deref(), repo.repo.as_deref()),
        },
        Some(Cmd::Script(c)) => match c {
            ScriptCmd::Edit(r) => script::edit(r.repo.as_deref()),
            ScriptCmd::Remove(r) => script::remove(r.repo.as_deref()),
            ScriptCmd::Show { repo, json } => script::show(repo.repo.as_deref(), json.json),
        },
        Some(Cmd::Doctor(j)) => doctor::run(j.json),
        Some(Cmd::Logs(a)) => logs::run(logs::Options {
            lines: a.lines.trim().parse().unwrap_or(50),
            follow: a.follow,
            level: a.level,
            module: a.module,
            clear: a.clear,
        }),
        Some(Cmd::Status { worktree, set }) => status::run(&worktree, set.as_deref()),
        // Normally caught by the fast path above (e.g. reached via `am-rs --watch hook-event`).
        Some(Cmd::HookEvent { worktree, event, managed: _ }) => am_hook::run_hook_event(&worktree, event.as_deref()),
    }
}

