//! `am` — the native CLI. Port of
//! `src/cli.tsx` + `src/commands/**`: same command names, aliases, flags,
//! output formats and exit codes. With no subcommand it execs `am-tui`.
//!
//! `hook-event` runs on every agent hook, so it is dispatched before
//! clap or any other initialisation and shares its code with `am-hook`.

am_core::silence_unit_tests!();

mod commands;
pub use am_core::hooks_installer;
mod mcp;
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
    bin_name = "am",
    version,
    about = "Agent Monitor — TUI dashboard for git worktrees & coding agents (Claude Code, Codex, Gemini CLI, Cursor, Copilot CLI, …)"
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
    /// Manage agent sessions per worktree
    #[command(subcommand, alias = "a")]
    Agent(AgentCmd),
    /// Manage tracked repositories
    #[command(subcommand)]
    Repo(RepoCmd),
    /// View and modify settings
    #[command(subcommand)]
    Settings(SettingsCmd),
    /// Manage agent hooks (Claude Code, Codex, Gemini CLI, Cursor, …)
    #[command(subcommand)]
    Hooks(HooksCmd),
    /// Install agent hooks (alias for: hooks install)
    InstallHooks(AgentOpt),
    /// Remove agent hooks (alias for: hooks uninstall)
    UninstallHooks(AgentOpt),
    /// Show PR info for a worktree branch
    #[command(args_conflicts_with_subcommands = true)]
    Pr(TargetCmd),
    /// Show Linear ticket info for a worktree branch
    #[command(args_conflicts_with_subcommands = true)]
    Linear(TargetCmd),
    /// Manage startup scripts
    #[command(subcommand)]
    Script(ScriptCmd),
    /// List, inspect and share colour themes
    #[command(subcommand)]
    Theme(ThemeCmd),
    /// MCP server that gives agents am's commands as tools (stdio); `am mcp install` registers it
    #[command(args_conflicts_with_subcommands = true)]
    Mcp {
        #[command(subcommand)]
        cmd: Option<McpCmd>,
    },
    /// Check system health (hooks, gh CLI, DB, etc.)
    Doctor(JsonOpt),
    /// Show recent logs
    Logs(LogsArgs),
    /// How long each part of the app has been taking (from the log), against its budget
    Perf(PerfArgs),
    /// Run the setup wizard
    Setup,
    /// Open only the review-requests page (appMode stays as it is)
    #[command(visible_alias = "review")]
    Reviews,
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
        /// Harness that sent the event (claude, gemini, codex, qwen, droid, copilot, cursor, codebuddy, or a plugin id); default claude
        #[arg(short, long, value_name = "id")]
        agent: Option<String>,
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
    /// Check out an existing branch in the Reviewing space (grouped apart, outside the worktree limit); never makes a new branch
    #[arg(long)]
    tmp: bool,
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
    /// Resume the worktree's latest agent session in this terminal (claude --resume, gemini --resume)
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
    /// Install hooks (~/.claude/settings.json, ~/.gemini/settings.json); default: every detected agent
    Install(AgentOpt),
    /// Remove agent-monitor hooks; default: from every agent
    Uninstall(AgentOpt),
    /// Check which agents have hooks installed
    Status(JsonOpt),
}

#[derive(Subcommand)]
enum McpCmd {
    /// Run the MCP server on stdin/stdout (the default; what agent CLIs launch)
    Serve,
    /// Register the MCP server with agent CLIs and Claude Desktop (`--agent claude-desktop`); default: every detected one that supports it
    Install(AgentOpt),
    /// Remove the MCP server registration; default: from every agent
    Uninstall(AgentOpt),
    /// Check which agents have the MCP server registered
    Status(JsonOpt),
}

#[derive(Args)]
struct AgentOpt {
    /// Only this agent (claude, gemini, codex, qwen, droid, copilot, cursor, codebuddy, …); repeatable
    #[arg(short, long = "agent", value_name = "id")]
    agents: Vec<String>,
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

#[derive(Args, Clone)]
struct ScriptOpt {
    #[command(flatten)]
    repo: RepoOpt,
    /// The review script (run by "pull + run script" on the review page) instead of the startup script
    #[arg(long)]
    review: bool,
}

#[derive(Subcommand)]
enum ScriptCmd {
    /// Create/open startup script in IDE
    Edit(ScriptOpt),
    /// Remove startup script
    Remove(ScriptOpt),
    /// Print startup script contents
    Show {
        #[command(flatten)]
        script: ScriptOpt,
        #[command(flatten)]
        json: JsonOpt,
    },
}

#[derive(Subcommand)]
enum ThemeCmd {
    /// List built-in and custom themes
    #[command(alias = "ls")]
    List(JsonOpt),
    /// Show a theme's colours
    Show {
        name: String,
        #[command(flatten)]
        json: JsonOpt,
    },
    /// Make a theme the active one
    Use { name: String },
    /// Print a theme as a standalone JSON file (a template for your own)
    Export {
        name: String,
        /// Write to a file instead of stdout
        #[arg(short, long, value_name = "file")]
        output: Option<String>,
    },
    /// Copy a theme file into the themes directory
    Import {
        file: String,
        /// Theme name (default: the file's "name", else its file name)
        #[arg(long, value_name = "name")]
        name: Option<String>,
        /// Replace an existing custom theme with the same name
        #[arg(long)]
        force: bool,
    },
    /// Delete a custom theme
    #[command(alias = "rm")]
    Remove { name: String },
}

#[derive(Args)]
struct PerfArgs {
    /// Window to report: 30m, 1h, 2d, or all
    #[arg(long, value_name = "window", default_value = "24h")]
    since: String,
    /// Output as JSON
    #[arg(long)]
    json: bool,
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
    /// Open the log in the configured editor (the log folder once it has rotated)
    #[arg(long, conflicts_with_all = ["follow", "clear"])]
    open: bool,
}

fn main() {
    // Fast path: no clap, no settings, no logger before the hook handler.
    let mut argv = std::env::args();
    if argv.nth(1).as_deref() == Some("hook-event") {
        match am_hook::parse_hook_args(argv) {
            Ok(a) => am_hook::run_hook_event(&a.worktree, a.event.as_deref(), a.agent.as_deref()),
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
        None => tui::launch(cli.watch, tui::Page::Dashboard),
        Some(Cmd::Setup) => tui::launch(false, tui::Page::Setup),
        Some(Cmd::Reviews) => tui::launch(false, tui::Page::Reviews),
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
            HooksCmd::Install(a) => hooks::install(&a.agents),
            HooksCmd::Uninstall(a) => hooks::uninstall(&a.agents),
            HooksCmd::Status(j) => hooks::status(j.json),
        },
        Some(Cmd::InstallHooks(a)) => hooks::install(&a.agents),
        Some(Cmd::UninstallHooks(a)) => hooks::uninstall(&a.agents),
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
            ScriptCmd::Edit(s) => script::edit(s.repo.repo.as_deref(), script::kind(s.review)),
            ScriptCmd::Remove(s) => script::remove(s.repo.repo.as_deref(), script::kind(s.review)),
            ScriptCmd::Show { script: s, json } => script::show(s.repo.repo.as_deref(), script::kind(s.review), json.json),
        },
        Some(Cmd::Theme(c)) => match c {
            ThemeCmd::List(j) => theme::list(j.json),
            ThemeCmd::Show { name, json } => theme::show(&name, json.json),
            ThemeCmd::Use { name } => theme::use_theme(&name),
            ThemeCmd::Export { name, output } => theme::export(&name, output.as_deref()),
            ThemeCmd::Import { file, name, force } => theme::import(&file, name.as_deref(), force),
            ThemeCmd::Remove { name } => theme::remove(&name),
        },
        Some(Cmd::Mcp { cmd }) => match cmd {
            None | Some(McpCmd::Serve) => crate::mcp::server::serve(),
            Some(McpCmd::Install(a)) => commands::mcp::install(&a.agents),
            Some(McpCmd::Uninstall(a)) => commands::mcp::uninstall(&a.agents),
            Some(McpCmd::Status(j)) => commands::mcp::status(j.json),
        },
        Some(Cmd::Doctor(j)) => doctor::run(j.json),
        Some(Cmd::Logs(a)) => logs::run(logs::Options {
            lines: a.lines.trim().parse().unwrap_or(50),
            follow: a.follow,
            level: a.level,
            module: a.module,
            clear: a.clear,
            open: a.open,
        }),
        Some(Cmd::Perf(a)) => perf::run(&a.since, a.json),
        Some(Cmd::Status { worktree, set }) => status::run(&worktree, set.as_deref()),
        // Normally caught by the fast path above (e.g. reached via `am --watch hook-event`).
        Some(Cmd::HookEvent { worktree, event, managed: _, agent }) => {
            am_hook::run_hook_event(&worktree, event.as_deref(), agent.as_deref())
        }
    }
}

