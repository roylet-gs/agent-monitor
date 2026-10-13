//! `am` — the native CLI. Port of
//! `src/cli.tsx` + `src/commands/**`: same command names, aliases, flags,
//! output formats and exit codes. With no subcommand it execs `am-tui`.
//!
//! `hook-event` runs on every agent hook, so it is dispatched before
//! clap or any other initialisation and shares its code with `am-hook`.

mod commands;
pub use am_core::hooks_installer;
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
    about = "Agent Monitor — TUI dashboard for git worktrees & coding agents (Claude Code, Gemini CLI)"
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
    /// Manage agent hooks (Claude Code, Gemini CLI)
    #[command(subcommand)]
    Hooks(HooksCmd),
    /// Harness plugins: list, validate, add, publish, and train new ones
    #[command(subcommand, alias = "plugin", alias = "plugins")]
    Harness(HarnessCmd),
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
        /// Agent CLI that sent the event (claude, gemini); default claude
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
enum HarnessCmd {
    /// List harnesses with their status (draft/published), hooks and issue counts
    #[command(alias = "ls")]
    List(JsonOpt),
    /// Show a harness: manifest, hook file, captures, issues
    Show {
        id: String,
        #[command(flatten)]
        json: JsonOpt,
    },
    /// Print the manifest reference (--json: the JSON Schema)
    Schema(JsonOpt),
    /// Validate a manifest file or a plugin id; lists every problem
    Validate {
        #[arg(value_name = "file|id")]
        target: String,
        #[command(flatten)]
        json: JsonOpt,
    },
    /// Add a manifest as a draft plugin and install its hooks
    Add {
        file: String,
        /// Replace an existing plugin with the same id (the old one is kept in the history)
        #[arg(long)]
        replace: bool,
        /// Keep the file's own status instead of making it a draft
        #[arg(long)]
        keep_status: bool,
        /// Don't install the hooks
        #[arg(long)]
        no_install: bool,
    },
    /// Hook events captured for a plugin (drafts capture every event)
    Captures {
        id: String,
        /// Only this native event
        #[arg(long, value_name = "name")]
        event: Option<String>,
        /// Only the newest N captures
        #[arg(long, value_name = "N")]
        last: Option<usize>,
        /// Print whole captures instead of a summary
        #[arg(long)]
        full: bool,
        /// Delete the captures
        #[arg(long)]
        clear: bool,
        #[command(flatten)]
        json: JsonOpt,
    },
    /// Replay captured events through a manifest and show the resulting statuses (writes nothing)
    Test {
        id: String,
        /// Candidate manifest to test instead of the installed one
        #[arg(long, value_name = "file")]
        manifest: Option<String>,
        /// Replay these capture files instead of all of the plugin's
        #[arg(long = "capture", value_name = "file")]
        captures: Vec<std::path::PathBuf>,
        #[command(flatten)]
        json: JsonOpt,
    },
    /// Problems recorded while a plugin was used
    Issues {
        id: String,
        /// Delete the plugin's issues
        #[arg(long)]
        clear: bool,
        #[command(flatten)]
        json: JsonOpt,
    },
    /// Mark a plugin published (needs a clean validation; stops capturing)
    Publish { id: String },
    /// Mark a plugin draft again (captures every event)
    Draft { id: String },
    /// Uninstall a plugin's hooks and remove it (kept in the history)
    Remove {
        id: String,
        /// Keep its captured events
        #[arg(long)]
        keep_captures: bool,
    },
    /// Restore a plugin's previous version
    Rollback { id: String },
    /// Start a training session: a terminal running the harness with a prompt to write or fix its plugin
    Train {
        /// How you start the harness to train (e.g. cursor-agent)
        #[arg(long, value_name = "cmd")]
        command: Option<String>,
        /// Retrain an existing plugin
        #[arg(long, value_name = "id")]
        edit: Option<String>,
        /// Print the prompt instead of starting
        #[arg(long)]
        print: bool,
        /// Prepare the workspace but don't open a terminal
        #[arg(long)]
        no_open: bool,
    },
}

#[derive(Args)]
struct AgentOpt {
    /// Only this agent (claude, gemini); repeatable
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
            HooksCmd::Install(a) => hooks::install(&a.agents),
            HooksCmd::Uninstall(a) => hooks::uninstall(&a.agents),
            HooksCmd::Status(j) => hooks::status(j.json),
        },
        Some(Cmd::Harness(h)) => match h {
            HarnessCmd::List(j) => harness::list(j.json),
            HarnessCmd::Show { id, json } => harness::show(&id, json.json),
            HarnessCmd::Schema(j) => harness::schema(j.json),
            HarnessCmd::Validate { target, json } => harness::validate(&target, json.json),
            HarnessCmd::Add { file, replace, keep_status, no_install } => harness::add(&file, replace, keep_status, no_install),
            HarnessCmd::Captures { id, event, last, full, clear, json } => harness::captures(
                &id,
                &harness::CapturesArgs { event: event.as_deref(), last, full, json: json.json, clear },
            ),
            HarnessCmd::Test { id, manifest, captures, json } => harness::test(&id, manifest.as_deref(), &captures, json.json),
            HarnessCmd::Issues { id, clear, json } => harness::issues(&id, json.json, clear),
            HarnessCmd::Publish { id } => harness::set_status(&id, am_core::harness::manifest::PluginStatus::Published),
            HarnessCmd::Draft { id } => harness::set_status(&id, am_core::harness::manifest::PluginStatus::Draft),
            HarnessCmd::Remove { id, keep_captures } => harness::remove(&id, keep_captures),
            HarnessCmd::Rollback { id } => harness::rollback(&id),
            HarnessCmd::Train { command, edit, print, no_open } => harness::train(command.as_deref(), edit.as_deref(), print, no_open),
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
            ScriptCmd::Edit(r) => script::edit(r.repo.as_deref()),
            ScriptCmd::Remove(r) => script::remove(r.repo.as_deref()),
            ScriptCmd::Show { repo, json } => script::show(repo.repo.as_deref(), json.json),
        },
        Some(Cmd::Theme(c)) => match c {
            ThemeCmd::List(j) => theme::list(j.json),
            ThemeCmd::Show { name, json } => theme::show(&name, json.json),
            ThemeCmd::Use { name } => theme::use_theme(&name),
            ThemeCmd::Export { name, output } => theme::export(&name, output.as_deref()),
            ThemeCmd::Import { file, name, force } => theme::import(&file, name.as_deref(), force),
            ThemeCmd::Remove { name } => theme::remove(&name),
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
        // Normally caught by the fast path above (e.g. reached via `am --watch hook-event`).
        Some(Cmd::HookEvent { worktree, event, managed: _, agent }) => {
            am_hook::run_hook_event(&worktree, event.as_deref(), agent.as_deref())
        }
    }
}

